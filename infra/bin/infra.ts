import { App } from 'aws-cdk-lib';
import { BenchStack } from '../lib/bench-stack.ts';

const app = new App();

new BenchStack(app, 'RunnerChessBench', {
  env: { account: process.env.CDK_DEFAULT_ACCOUNT, region: process.env.CDK_DEFAULT_REGION },
  vpcId: app.node.tryGetContext('vpcId'),
  ref: app.node.getContext('ref'),
  seconds: Number(app.node.getContext('seconds')),
});
