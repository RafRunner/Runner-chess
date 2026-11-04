import { App } from 'aws-cdk-lib';
import { BenchStack } from '../lib/bench-stack.ts';
import { DatagenStack } from '../lib/datagen-stack.ts';

const app = new App();
const env = { account: process.env.CDK_DEFAULT_ACCOUNT, region: process.env.CDK_DEFAULT_REGION };
const vpcId = app.node.tryGetContext('vpcId');
const ref = app.node.getContext('ref');

new BenchStack(app, 'RunnerChessBench', {
  env,
  vpcId,
  ref,
  seconds: Number(app.node.getContext('seconds')),
});

new DatagenStack(app, 'RunnerChessDatagen', {
  env,
  vpcId,
  ref,
  bench: Number(app.node.getContext('bench')),
  workerTypes: String(app.node.getContext('workerTypes')).split(','),
  maxWorkers: Number(app.node.getContext('maxWorkers')),
});
