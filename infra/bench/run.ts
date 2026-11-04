/**
 * Launches a Spot instance of each type given, or of DEFAULT_TYPES, from the launch
 * templates of RunnerChessBench, waits for what bench/instance.sh leaves in S3, and
 * prints how many positions a second each made and what 100 million would cost at the
 * Spot price of its zone, the cheapest one with capacity, when it was launched.
 *
 *   npm run bench -- [instance types...]
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CloudFormationClient, DescribeStacksCommand } from '@aws-sdk/client-cloudformation';
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

const STACK = 'RunnerChessBench';
// 4 vCPUs each, 20 together, within the smallest Spot quotas: a vCPU makes as many
// positions in every size of a family
const DEFAULT_TYPES = ['c6g.xlarge', 'c7g.xlarge', 'c8g.xlarge', 'c7a.xlarge', 'c7i.xlarge'];
/** `bench search` of 1.7.0: any other count means the search differs on that CPU */
const EXPECTED_BENCH = 22792;
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
  const types = process.argv.length > 2 ? process.argv.slice(2) : DEFAULT_TYPES;
  const outputs = await stackOutputs();
  const bucket = outputs.ResultsBucket;
  const runId = new Date().toISOString().replace(/[:.]/g, '-');

  const runs = await describe(types);
  const subnets = outputs.Subnets.split(',');
  const described = await ec2.send(new DescribeSubnetsCommand({ SubnetIds: subnets }));
  const zones = new Map(described.Subnets?.map((s) => [s.SubnetId!, s.AvailabilityZone!]));
  for (const run of runs) {
    await launch(run, outputs, zones, runId);
  }

  const started = Date.now();
  const pending = new Set(runs.filter((run) => run.instanceId));
  while (pending.size > 0 && Date.now() - started < DEADLINE_MS) {
    await new Promise((resolve) => setTimeout(resolve, POLL_MS));
    for (const run of [...pending]) {
      // the state first: an instance that terminated after leaving its results has them
      const terminated = (await state(run.instanceId!)) === 'terminated';
      const files = await list(bucket, `${runId}/${run.type}/`);
      if (files.includes('done')) {
        await readResults(run, bucket, `${runId}/${run.type}/`);
      } else if (files.includes('instance.log')) {
        run.error = `failed, see s3://${bucket}/${runId}/${run.type}/instance.log`;
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

async function stackOutputs(): Promise<Record<string, string>> {
  const response = await new CloudFormationClient({}).send(
    new DescribeStacksCommand({ StackName: STACK }),
  );
  const outputs = response.Stacks?.[0]?.Outputs ?? [];
  return Object.fromEntries(outputs.map((output) => [output.OutputKey!, output.OutputValue!]));
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
) {
  const template = run.arch === 'arm64' ? outputs.ArmLaunchTemplate : outputs.X86LaunchTemplate;
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
  console.log('\n| type | arch | vCPUs | bench | positions/s | per vCPU | Spot $/h | $ per 100M |');
  console.log('|---|---|---|---|---|---|---|---|');
  for (const run of runs) {
    if (run.error || !run.positionsPerSecond) {
      console.log(`| ${run.type} | ${run.arch} | ${run.vcpus} | ${run.error ?? 'no rate'} | | | | |`);
      continue;
    }
    const bench = run.benchNodes === EXPECTED_BENCH ? `${run.benchNodes}` : `**${run.benchNodes}**`;
    const perVcpu = run.positionsPerSecond / run.vcpus;
    const cost = run.spotPrice && (run.spotPrice * 1e8) / (run.positionsPerSecond * 3600);
    console.log(
      `| ${run.type} | ${run.arch} | ${run.vcpus} | ${bench} | ${run.positionsPerSecond} | ` +
        `${perVcpu.toFixed(0)} | ${run.spotPrice?.toFixed(4) ?? '?'} | ${cost ? cost.toFixed(2) : '?'} |`,
    );
  }
}

await main();
