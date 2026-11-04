import { App } from 'aws-cdk-lib';
import { RunnerChessStack } from '../lib/runner-chess-stack.ts';

const app = new App();

new RunnerChessStack(app, 'RunnerChess', {
  env: { account: process.env.CDK_DEFAULT_ACCOUNT, region: process.env.CDK_DEFAULT_REGION },
  vpcId: app.node.tryGetContext('vpcId'),
  workerTypes: String(app.node.getContext('workerTypes')).split(','),
  maxWorkers: Number(app.node.getContext('maxWorkers')),
});
