/**
 * The jobs of the workers of RunnerChess, from here, one at a time:
 *
 *   npm run job -- datagen <job file> [--workers <n>] [--hours <n>] [--name <job>] [--force]
 *   npm run job -- sprt <engine> <baseline> [--tc 10+0.1] [--bounds 0,10] [--hours <n>]
 *       [--name <job>] [--engine-name <name>] [--baseline-name <name>]
 *       [--reference-nps <n>] [--force]
 *   npm run job -- status
 *   npm run job -- stop
 *   npm run job -- fetch [<job>]
 *
 * A job doesn't begin while workers still play another, unless `--force`.
 *
 * `datagen` plays the parts of a job file, as in worker/jobs/, with every worker the
 * group can have by default, unless the bucket already has every position of it.
 * `--name` gives the job of the file another name, and so other chunks.
 *
 * `sprt` plays a match of fastchess on one worker until the SPRT decides, as
 * scripts/match.sh --sprt does here. The engine and the baseline are git refs, or
 * `worktree` for the working tree as it is, a network not committed included. The time
 * control is the one of this computer: each worker scales it by how much slower it
 * searches than the baseline does here, `engines/runner-chess-<baseline>`, or than
 * `--reference-nps`.
 *
 * `fetch` downloads the chunks of a datagen job, the current one by default, into
 * data/<job>-<part>/ as the text of datagen, or the games and the output of an SPRT
 * into engines/games/<job>.pgn and .log.
 */
import { execFileSync } from 'node:child_process';
import {
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from 'node:fs';
import { basename, join } from 'node:path';
import type { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { createZstdDecompress } from 'node:zlib';
import {
  AutoScalingClient,
  DescribeAutoScalingGroupsCommand,
  SetDesiredCapacityCommand,
} from '@aws-sdk/client-auto-scaling';
import {
  GetObjectCommand,
  ListObjectsV2Command,
  PutObjectCommand,
  S3Client,
  type _Object,
} from '@aws-sdk/client-s3';
import { repo, stackOutputs, uploadSource } from '../lib/runner-aws.ts';
import { type Cell, table } from '../lib/table.ts';

/** The chunks of the last half hour tell how fast a part goes */
const RATE_WINDOW_MS = 30 * 60_000;
/** The depth of `bench search` whose nodes a second compare the speed of two CPUs */
const NPS_DEPTH = 17;
/** A date and time of here, as 2026-11-04 00:12:19 */
const local = (date: string | Date) => new Date(date).toLocaleString('sv-SE');

interface Part {
  name: string;
  positions: number;
  nodes: number;
  /** A path from the repository here, a key under books/ in the bucket */
  book?: string;
}

interface Engine {
  name: string;
  /** A git ref, or `worktree` */
  source: string;
  sourceKey?: string;
}

type Job = {
  name: string;
  until?: string;
  stop?: boolean;
} & (
  | {
      kind: 'datagen';
      source: string;
      sourceKey?: string;
      /** The nodes of `bench search` of the source */
      bench: number;
      /** Positions a chunk, between the threads of a worker */
      chunk: number;
      parts: Part[];
    }
  | {
      kind: 'sprt';
      engine: Engine;
      baseline: Engine;
      /** `<seconds>+<increment>`, at the speed of `referenceNps` */
      tc: string;
      bounds: [number, number];
      book: string;
      referenceNps: number;
      npsDepth: number;
    }
);

const s3 = new S3Client({});
const autoscaling = new AutoScalingClient({});

async function main() {
  const [command, ...args] = process.argv.slice(2);
  const outputs = await stackOutputs();
  const bucket = outputs.Bucket;
  const group = outputs.WorkersGroup;
  const flags = ['--force'];
  const option = (name: string) => {
    const i = args.indexOf(`--${name}`);
    return i < 0 ? undefined : args[i + 1];
  };
  // what isn't an option nor the value of one
  const positional = args.filter(
    (arg, i) =>
      !arg.startsWith('--') && !(args[i - 1]?.startsWith('--') && !flags.includes(args[i - 1])),
  );

  switch (command) {
    case 'datagen':
      return datagen(bucket, group, positional[0], option);
    case 'sprt':
      return sprt(bucket, group, positional, option);
    case 'status':
      return status(bucket, group);
    case 'stop': {
      const job = await readJob(bucket);
      await writeJob(bucket, { ...job, stop: true });
      await desire(group, 0);
      console.log(`${job.name} stopped, the workers terminate`);
      return;
    }
    case 'fetch':
      return fetch(bucket, positional[0]);
    default:
      console.error(readFileSync(import.meta.filename, 'utf8').split('*/')[0]);
      process.exitCode = 1;
  }
}

type Option = (name: string) => string | undefined;

async function datagen(bucket: string, group: string, file: string | undefined, option: Option) {
  if (!file) {
    throw new Error('datagen needs a job file');
  }
  const job: Job = JSON.parse(readFileSync(file, 'utf8'));
  if (job.kind !== 'datagen') {
    throw new Error(`${file} is not a datagen job`);
  }
  job.name = option('name') ?? job.name;

  // the workers would only build the engine and leave
  const missing = [];
  for (const part of job.parts) {
    if (sum(await list(bucket, `datagen/${job.name}/${part.name}/`)) < part.positions) {
      missing.push(part.name);
    }
  }
  if (missing.length === 0) {
    console.log(`${job.name} already has every position: give it another --name to play again`);
    return;
  }

  for (const part of job.parts) {
    if (part.book) {
      part.book = await uploadBook(bucket, part.book);
    }
  }
  job.sourceKey = await uploadSource(s3, bucket, job.source);
  const workers = Number(option('workers') ?? (await describeGroup(group)).MaxSize!);
  await begin(bucket, group, job, workers, option);
}

async function sprt(bucket: string, group: string, sources: string[], option: Option) {
  const [engine, baseline] = sources;
  if (!engine || !baseline) {
    throw new Error('sprt needs an engine and a baseline');
  }
  const engineName = option('engine-name') ?? engine;
  const baselineName = option('baseline-name') ?? baseline;
  const job: Job = {
    kind: 'sprt',
    name: option('name') ?? `${engineName}-vs-${baselineName}-${local(new Date()).replace(/\D/g, '')}`,
    engine: { name: engineName, source: engine },
    baseline: { name: baselineName, source: baseline },
    tc: option('tc') ?? '10+0.1',
    bounds: (option('bounds') ?? '0,10').split(',').map(Number) as [number, number],
    book: await uploadBook(bucket, 'engines/books/8moves_v3.pgn'),
    referenceNps: Number(option('reference-nps') ?? referenceNps(baselineName)),
    npsDepth: NPS_DEPTH,
  };
  job.engine.sourceKey = await uploadSource(s3, bucket, engine);
  job.baseline.sourceKey = await uploadSource(s3, bucket, baseline);
  // one fastchess plays the whole match
  await begin(bucket, group, job, 1, option);
}

/** The nodes a second of the baseline here, at the depth the workers measure them */
function referenceNps(baseline: string): number {
  const engine = join(repo, 'engines', `runner-chess-${baseline}`);
  if (!existsSync(engine)) {
    throw new Error(`no ${engine} to measure the speed of here: give --reference-nps`);
  }
  const out = execFileSync(engine, ['bench', 'search', String(NPS_DEPTH)], { encoding: 'utf8' });
  return Number(/Nodes\/second\s*:\s*(\d+)/.exec(out)![1]);
}

/** The job in the bucket with a deadline, and its workers, unless another job has some */
async function begin(bucket: string, group: string, job: Job, workers: number, option: Option) {
  const running = (await describeGroup(group)).Instances ?? [];
  if (running.length > 0 && !process.argv.includes('--force')) {
    throw new Error(`${running.length} workers still play ${(await readJob(bucket)).name}`);
  }
  const hours = Number(option('hours') ?? 24);
  job.until = new Date(Date.now() + hours * 3_600_000).toISOString();
  job.stop = false;
  await writeJob(bucket, job);
  await desire(group, workers);
  console.log(`${job.name}: ${workers} workers until ${local(job.until)}`);
}

async function status(bucket: string, group: string) {
  const job = await readJob(bucket);
  console.log(`${job.kind} ${job.name}, until ${local(job.until!)}${job.stop ? ', stopped' : ''}\n`);
  if (job.kind === 'datagen') {
    const now = Date.now();
    const rows: Cell[][] = [];
    for (const part of job.parts) {
      const chunks = await list(bucket, `datagen/${job.name}/${part.name}/`);
      const played = sum(chunks);
      const rate = recentRate(chunks, now);
      const left = Math.max(0, part.positions - played);
      const eta = left === 0 ? 'done' : rate > 0 ? `${(left / rate / 3600).toFixed(1)} h` : '?';
      const done = `${Math.floor((100 * played) / part.positions)}%`;
      const perSecond = rate > 0 ? Math.round(rate) : undefined;
      rows.push([part.name, played, part.positions, done, chunks.length, perSecond, eta]);
    }
    const headers = ['part', 'positions', 'of', 'done', 'chunks', 'per second', 'to go'];
    console.log(table(headers, rows, ['done', 'to go']));
  } else {
    console.log(`${job.engine.name} against ${job.baseline.name}, ${job.tc} here`);
    const results = (await readText(bucket, `sprt/${job.name}/fastchess.log`)) ?? '';
    const last = results.split('\n').filter((line) => /^(Elo|Games|LLR|Ptnml|SPRT)/.test(line));
    console.log(last.length > 0 ? last.slice(-5).join('\n') : 'no results yet');
  }

  const workers = await describeGroup(group);
  const instances = workers.Instances ?? [];
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
  console.log(`\n${plural(instances.length, 'worker')}, ${workers.DesiredCapacity} desired`);
  if (instances.length > 0) {
    const rows = instances.map((i) => [
      i.InstanceId,
      i.InstanceType,
      i.AvailabilityZone,
      i.LifecycleState,
    ]);
    console.log(`\n${table(['worker', 'type', 'zone', 'state'], rows)}`);
  }
}

async function fetch(bucket: string, name: string | undefined) {
  name ??= (await readJob(bucket)).name;
  const sprt = await list(bucket, `sprt/${name}/`);
  if (sprt.length > 0) {
    const games = join(repo, 'engines', 'games');
    mkdirSync(games, { recursive: true });
    for (const [file, local] of [
      ['games.pgn', `${name}.pgn`],
      ['fastchess.log', `${name}.log`],
    ]) {
      const text = await readText(bucket, `sprt/${name}/${file}`);
      if (text !== undefined) {
        writeFileSync(join(games, local), text);
      }
    }
    console.log(`the games and the output of ${name} in engines/games/`);
    return;
  }

  // a fetch skips the chunks already here, so it can resume
  const chunks = await list(bucket, `datagen/${name}/`);
  let fetched = 0;
  for (const chunk of chunks) {
    const [, , part, file] = chunk.Key!.split('/');
    const dir = join(repo, 'data', `${name}-${part}`);
    const out = join(dir, file.replace(/\.zst$/, ''));
    if (existsSync(out)) {
      continue;
    }
    mkdirSync(dir, { recursive: true });
    const response = await s3.send(new GetObjectCommand({ Bucket: bucket, Key: chunk.Key }));
    // written aside first, so that an interrupted fetch leaves no half chunk
    await pipeline(response.Body as Readable, createZstdDecompress(), createWriteStream(`${out}.part`));
    renameSync(`${out}.part`, out);
    fetched++;
  }
  console.log(
    `${fetched} chunks fetched, ${chunks.length} in ${name}, ` +
      `${sum(chunks).toLocaleString('en-US')} positions`,
  );
}

/** Uploads a book of the repository under books/, and gives its name there */
async function uploadBook(bucket: string, path: string): Promise<string> {
  const name = basename(path);
  await s3.send(
    new PutObjectCommand({ Bucket: bucket, Key: `books/${name}`, Body: readFileSync(join(repo, path)) }),
  );
  return name;
}

async function readJob(bucket: string): Promise<Job> {
  return JSON.parse((await readText(bucket, 'job.json'))!);
}

async function writeJob(bucket: string, job: Job) {
  await s3.send(
    new PutObjectCommand({
      Bucket: bucket,
      Key: 'job.json',
      Body: JSON.stringify(job, null, 2),
      ContentType: 'application/json',
    }),
  );
}

async function readText(bucket: string, key: string): Promise<string | undefined> {
  try {
    const response = await s3.send(new GetObjectCommand({ Bucket: bucket, Key: key }));
    return await response.Body!.transformToString();
  } catch (e) {
    if ((e as Error).name === 'NoSuchKey') {
      return undefined;
    }
    throw e;
  }
}

async function describeGroup(group: string) {
  const response = await autoscaling.send(
    new DescribeAutoScalingGroupsCommand({ AutoScalingGroupNames: [group] }),
  );
  return response.AutoScalingGroups![0];
}

async function desire(group: string, workers: number) {
  await autoscaling.send(
    new SetDesiredCapacityCommand({ AutoScalingGroupName: group, DesiredCapacity: workers }),
  );
}

async function list(bucket: string, prefix: string): Promise<_Object[]> {
  const objects: _Object[] = [];
  let token: string | undefined;
  do {
    const response = await s3.send(
      new ListObjectsV2Command({ Bucket: bucket, Prefix: prefix, ContinuationToken: token }),
    );
    objects.push(...(response.Contents ?? []));
    token = response.NextContinuationToken;
  } while (token);
  return objects;
}

/**
 * Positions a second, from the chunks of the last half hour: all but the first, over the
 * time from the first to the last, as the first one's time went before it. 0 with fewer
 * than two.
 */
function recentRate(chunks: _Object[], now: number): number {
  const recent = chunks
    .filter((c) => now - c.LastModified!.getTime() < RATE_WINDOW_MS)
    .sort((a, b) => a.LastModified!.getTime() - b.LastModified!.getTime());
  if (recent.length < 2) {
    return 0;
  }
  const span = recent.at(-1)!.LastModified!.getTime() - recent[0].LastModified!.getTime();
  return span > 0 ? sum(recent.slice(1)) / (span / 1000) : 0;
}

/** The positions of chunks, from their names: <seed>-<positions>.txt.zst */
function sum(chunks: _Object[]): number {
  return chunks.reduce((total, c) => total + Number(/-(\d+)\.txt\.zst$/.exec(c.Key!)?.[1] ?? 0), 0);
}

await main();
