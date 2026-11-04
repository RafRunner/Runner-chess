/**
 * Launches a Spot instance of each type given, or of DEFAULT_TYPES, from the launch
 * templates of RunnerChess, waits for what bench/instance.py leaves in the bucket, under
 * bench/<run>/<type>/, and prints how many positions a second each made and what 100
 * million would cost at the Spot price of its zone, the cheapest one with capacity, when
 * it was launched. The engine is the one of `--source`, a git ref or `worktree`, the
 * last tag by default.
 *
 *   npm run bench -- [--source <ref>] [instance types...]
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  DescribeInstancesCommand,
  DescribeInstanceTypesCommand,
  DescribeSpotPriceHistoryCommand,
  DescribeSubnetsCommand,
  EC2Client,
  RunInstancesCommand,
  type _InstanceType,
} from '@aws-sdk/client-ec2';
import { GetObjectCommand, ListObjectsV2Command, S3Client } from '@aws-sdk/client-s3';
import { repo, stackOutputs, uploadSource } from '../lib/runner-aws.ts';
import { type Cell, table } from '../lib/table.ts';

// 4 vCPUs each, 20 together, within the smallest Spot quotas: a vCPU makes as many
// positions in every size of a family
const DEFAULT_TYPES = ['c6g.xlarge', 'c7g.xlarge', 'c8g.xlarge', 'c7a.xlarge', 'c7i.xlarge'];
const POLL_MS = 30_000;
/** The instances shut themselves down after 45 minutes */
const DEADLINE_MS = 50 * 60_000;

interface Run {
  type: string;
  arch: 'arm64' | 'x86_64';
  vcpus: number;
  instanceId?: string;
  zone?: string;
  error?: string;
  benchNodes?: number;
  positionsPerSecond?: number;
  spotPrice?: number;
}

const ec2 = new EC2Client({});
const s3 = new S3Client({});

async function main() {
  const args = process.argv.slice(2);
  const at = args.indexOf('--source');
  const source =
    at < 0
      ? execFileSync('git', ['-C', repo, 'describe', '--tags', '--abbrev=0'], { encoding: 'utf8' }).trim()
      : args[at + 1];
  const given = at < 0 ? args : [...args.slice(0, at), ...args.slice(at + 2)];
  const types = given.length > 0 ? given : DEFAULT_TYPES;
  const outputs = await stackOutputs();
  const bucket = outputs.Bucket;
  const runId = new Date().toISOString().replace(/[:.]/g, '-');
  const sourceKey = await uploadSource(s3, bucket, source);
  console.log(`${source}, as ${sourceKey}`);

  const runs = await describe(types);
  const subnets = outputs.Subnets.split(',');
  const described = await ec2.send(new DescribeSubnetsCommand({ SubnetIds: subnets }));
  const zones = new Map(described.Subnets?.map((s) => [s.SubnetId!, s.AvailabilityZone!]));
  for (const run of runs) {
    await launch(run, outputs, zones, runId, sourceKey);
  }

  const started = Date.now();
  const pending = new Set(runs.filter((run) => run.instanceId));
  while (pending.size > 0 && Date.now() - started < DEADLINE_MS) {
    await new Promise((resolve) => setTimeout(resolve, POLL_MS));
    for (const run of [...pending]) {
      // the state first: an instance that terminated after leaving its results has them
      const terminated = (await state(run.instanceId!)) === 'terminated';
      const prefix = `bench/${runId}/${run.type}/`;
      const files = await list(bucket, prefix);
      if (files.includes('done')) {
        await readResults(run, bucket, prefix);
      } else if (files.includes('instance.log')) {
        run.error = `failed, see s3://${bucket}/${prefix}instance.log`;
      } else if (terminated) {
        run.error = 'terminated without results, maybe reclaimed';
      } else {
        continue;
      }
      pending.delete(run);
      console.log(`${run.type}: ${run.error ?? 'done'}`);
    }
  }
  for (const run of pending) {
    run.error = 'no results before the deadline';
  }

  report(runs);
  const dir = join(import.meta.dirname, '..', '.build');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `bench-${runId}.json`), JSON.stringify(runs, null, 2));
}

async function describe(types: string[]): Promise<Run[]> {
  const response = await ec2.send(
    new DescribeInstanceTypesCommand({ InstanceTypes: types as _InstanceType[] }),
  );
  return types.map((type) => {
    const info = response.InstanceTypes?.find((info) => info.InstanceType === type);
    if (!info) {
      throw new Error(`no instance type ${type} in this region`);
    }
    const arm = info.ProcessorInfo?.SupportedArchitectures?.includes('arm64');
    return { type, arch: arm ? 'arm64' : 'x86_64', vcpus: info.VCpuInfo?.DefaultVCpus ?? 0 };
  });
}

/** In the cheapest zone that has the capacity */
async function launch(
  run: Run,
  outputs: Record<string, string>,
  zones: Map<string, string>,
  runId: string,
  sourceKey: string,
) {
  const template =
    run.arch === 'arm64' ? outputs.BenchArmLaunchTemplate : outputs.BenchX86LaunchTemplate;
  const prices = await spotPrices(run.type);
  const price = (subnet: string) => prices.get(zones.get(subnet)!) ?? Infinity;
  const subnets = [...zones.keys()].sort((a, b) => price(a) - price(b));
  for (const subnet of subnets) {
    try {
      const response = await ec2.send(
        new RunInstancesCommand({
          LaunchTemplate: { LaunchTemplateId: template, Version: '$Latest' },
          InstanceType: run.type as _InstanceType,
          SubnetId: subnet,
          MinCount: 1,
          MaxCount: 1,
          TagSpecifications: [
            {
              ResourceType: 'instance',
              Tags: [
                { Key: 'Name', Value: `runner-chess-bench-${run.type}` },
                { Key: 'Run', Value: runId },
                // what the instance builds, a key in the bucket
                { Key: 'Source', Value: sourceKey },
              ],
            },
          ],
        }),
      );
      const instance = response.Instances?.[0];
      run.instanceId = instance?.InstanceId;
      run.zone = instance?.Placement?.AvailabilityZone;
      run.spotPrice = prices.get(run.zone!);
      run.error = undefined;
      console.log(`${run.type}: ${run.instanceId} in ${run.zone}, $${run.spotPrice}/h`);
      return;
    } catch (e) {
      run.error = `${(e as Error).name}: ${(e as Error).message}`;
    }
  }
  console.log(`${run.type}: not launched, ${run.error}`);
}

async function state(instanceId: string): Promise<string | undefined> {
  const response = await ec2.send(new DescribeInstancesCommand({ InstanceIds: [instanceId] }));
  return response.Reservations?.[0]?.Instances?.[0]?.State?.Name;
}

async function list(bucket: string, prefix: string): Promise<string[]> {
  const response = await s3.send(new ListObjectsV2Command({ Bucket: bucket, Prefix: prefix }));
  return (response.Contents ?? []).map((object) => object.Key!.slice(prefix.length));
}

async function read(bucket: string, key: string): Promise<string> {
  const response = await s3.send(new GetObjectCommand({ Bucket: bucket, Key: key }));
  return response.Body!.transformToString();
}

async function readResults(run: Run, bucket: string, prefix: string) {
  const bench = await read(bucket, `${prefix}bench.txt`);
  run.benchNodes = Number(/Nodes searched\s*:\s*(\d+)/.exec(bench)?.[1]);
  // the last report of datagen has the rate over the whole run
  const last = (await read(bucket, `${prefix}datagen.txt`)).trim().split('\n').at(-1) ?? '';
  run.positionsPerSecond = Number(/(\d+) positions\/s/.exec(last)?.[1]);
}

/** The Spot price of `type` now, in dollars an hour, by zone */
async function spotPrices(type: string): Promise<Map<string, number>> {
  const response = await ec2.send(
    new DescribeSpotPriceHistoryCommand({
      InstanceTypes: [type as _InstanceType],
      ProductDescriptions: ['Linux/UNIX'],
      StartTime: new Date(),
    }),
  );
  return new Map(
    (response.SpotPriceHistory ?? []).map((p) => [p.AvailabilityZone!, Number(p.SpotPrice)]),
  );
}

function report(runs: Run[]) {
  const measured = runs.filter((run) => !run.error && run.positionsPerSecond);
  // every CPU must search the same tree: the count most of them got is the right one
  const frequency = new Map<number | undefined, number>();
  for (const run of measured) {
    frequency.set(run.benchNodes, (frequency.get(run.benchNodes) ?? 0) + 1);
  }
  const usual = [...frequency].sort((a, b) => b[1] - a[1])[0]?.[0];
  const rows: Cell[][] = measured.map((run) => {
    const rate = run.positionsPerSecond!;
    const bench = run.benchNodes === usual ? run.benchNodes : `${run.benchNodes}, not ${usual}`;
    const cost = run.spotPrice && (run.spotPrice * 1e8) / (rate * 3600);
    return [
      run.type,
      run.arch,
      run.vcpus,
      bench,
      rate,
      Math.round(rate / run.vcpus),
      run.spotPrice?.toFixed(4),
      cost?.toFixed(2),
    ];
  });
  const headers = ['type', 'arch', 'vCPUs', 'bench', 'positions/s', 'per vCPU', 'Spot $/h', '$ per 100M'];
  console.log(`\n${table(headers, rows, ['bench', 'Spot $/h', '$ per 100M'])}`);
  for (const run of runs.filter((run) => !measured.includes(run))) {
    console.log(`${run.type}: ${run.error ?? 'no rate in its results'}`);
  }
}

await main();
