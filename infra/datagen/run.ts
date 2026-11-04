/**
 * The datagen workers of RunnerChessDatagen, from here:
 *
 *   npm run datagen -- start <job file> [--workers <n>] [--hours <n>]
 *   npm run datagen -- status
 *   npm run datagen -- stop
 *   npm run datagen -- fetch [<job>]
 *
 * `start` uploads the books of the job and the job itself, with a deadline, and asks for
 * the workers, every one the group can have by default. `fetch` downloads every chunk of
 * a job, the current one by default, into data/<job>-<part>/, as the text of datagen.
 */
import { createWriteStream, existsSync, mkdirSync, readFileSync, renameSync } from 'node:fs';
import { basename, join } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { createZstdDecompress } from 'node:zlib';
import {
  AutoScalingClient,
  DescribeAutoScalingGroupsCommand,
  SetDesiredCapacityCommand,
} from '@aws-sdk/client-auto-scaling';
import { CloudFormationClient, DescribeStacksCommand } from '@aws-sdk/client-cloudformation';
import {
  GetObjectCommand,
  ListObjectsV2Command,
  PutObjectCommand,
  S3Client,
  type _Object,
} from '@aws-sdk/client-s3';
import { type Cell, table } from '../lib/table.ts';

const STACK = 'RunnerChessDatagen';
const repo = join(import.meta.dirname, '..', '..');
/** The chunks of the last half hour tell how fast a part goes */
const RATE_WINDOW_MS = 30 * 60_000;
/** A date and time of here, as 2026-11-04 00:12:19 */
const local = (date: string | Date) => new Date(date).toLocaleString('sv-SE');

interface Part {
  name: string;
  positions: number;
  nodes: number;
  /** A path from the repository here, a key under books/ in the bucket */
  book?: string;
}

interface Job {
  name: string;
  /** Positions a chunk, between the threads of a worker */
  chunk: number;
  parts: Part[];
  until?: string;
  stop?: boolean;
}

const s3 = new S3Client({});
const autoscaling = new AutoScalingClient({});

async function main() {
  const [command, ...args] = process.argv.slice(2);
  const outputs = await stackOutputs();
  const bucket = outputs.DataBucket;
  const group = outputs.WorkersGroup;

  switch (command) {
    case 'start':
      return start(bucket, group, args);
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
      return fetch(bucket, args[0] ?? (await readJob(bucket)).name);
    default:
      console.error('usage: npm run datagen -- start <job file> [--workers <n>] [--hours <n>]');
      console.error('       npm run datagen -- status | stop | fetch [<job>]');
      process.exitCode = 1;
  }
}

async function start(bucket: string, group: string, args: string[]) {
  const [file] = args;
  if (!file) {
    throw new Error('start needs a job file');
  }
  const option = (name: string) => {
    const i = args.indexOf(`--${name}`);
    return i < 0 ? undefined : Number(args[i + 1]);
  };
  const job: Job = JSON.parse(readFileSync(file, 'utf8'));
  for (const part of job.parts) {
    if (part.book) {
      const key = basename(part.book);
      await s3.send(
        new PutObjectCommand({
          Bucket: bucket,
          Key: `books/${key}`,
          Body: readFileSync(join(repo, part.book)),
        }),
      );
      part.book = key;
    }
  }
  const hours = option('hours') ?? 24;
  job.until = new Date(Date.now() + hours * 3_600_000).toISOString();
  job.stop = false;
  await writeJob(bucket, job);

  const workers = option('workers') ?? (await describeGroup(group)).MaxSize!;
  await desire(group, workers);
  console.log(`${job.name}: ${workers} workers until ${local(job.until)}`);
}

async function status(bucket: string, group: string) {
  const job = await readJob(bucket);
  const now = Date.now();
  console.log(`${job.name}, until ${local(job.until!)}${job.stop ? ', stopped' : ''}\n`);
  const rows: Cell[][] = [];
  for (const part of job.parts) {
    const chunks = await list(bucket, `${job.name}/${part.name}/`);
    const played = sum(chunks);
    const rate = recentRate(chunks, now);
    const left = Math.max(0, part.positions - played);
    const eta = left === 0 ? 'done' : rate > 0 ? `${(left / rate / 3600).toFixed(1)} h` : '?';
    const done = `${Math.floor((100 * played) / part.positions)}%`;
    rows.push([part.name, played, part.positions, done, chunks.length, rate > 0 ? Math.round(rate) : undefined, eta]);
  }
  const headers = ['part', 'positions', 'of', 'done', 'chunks', 'per second', 'to go'];
  console.log(table(headers, rows, ['done', 'to go']));

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

/** Skips the chunks already here: a fetch can resume */
async function fetch(bucket: string, name: string) {
  const chunks = await list(bucket, `${name}/`);
  let fetched = 0;
  for (const chunk of chunks) {
    const [, part, file] = chunk.Key!.split('/');
    const dir = join(repo, 'data', `${name}-${part}`);
    const out = join(dir, file.replace(/\.zst$/, ''));
    if (existsSync(out)) {
      continue;
    }
    mkdirSync(dir, { recursive: true });
    const response = await s3.send(new GetObjectCommand({ Bucket: bucket, Key: chunk.Key }));
    // written aside first, so that an interrupted fetch leaves no half chunk
    await pipeline(
      response.Body as Readable,
      createZstdDecompress(),
      createWriteStream(`${out}.part`),
    );
    renameSync(`${out}.part`, out);
    fetched++;
  }
  console.log(
    `${fetched} chunks fetched, ${chunks.length} in ${name}, ` +
      `${sum(chunks).toLocaleString('en-US')} positions`,
  );
}

async function stackOutputs(): Promise<Record<string, string>> {
  const response = await new CloudFormationClient({}).send(
    new DescribeStacksCommand({ StackName: STACK }),
  );
  const outputs = response.Stacks?.[0]?.Outputs ?? [];
  return Object.fromEntries(outputs.map((output) => [output.OutputKey!, output.OutputValue!]));
}

async function readJob(bucket: string): Promise<Job> {
  const response = await s3.send(new GetObjectCommand({ Bucket: bucket, Key: 'job.json' }));
  return JSON.parse(await response.Body!.transformToString());
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
