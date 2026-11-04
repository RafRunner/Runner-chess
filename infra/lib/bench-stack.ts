import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { CfnOutput, RemovalPolicy, Stack, type StackProps } from 'aws-cdk-lib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as s3 from 'aws-cdk-lib/aws-s3';
import { Asset } from 'aws-cdk-lib/aws-s3-assets';
import type { Construct } from 'constructs';

export interface BenchStackProps extends StackProps {
  /** The VPC the instances run in, the default one of the region without it */
  vpcId?: string;
  /** The git ref of the engine to measure, as `git archive` takes it */
  ref: string;
  /** How long `datagen` plays on each instance */
  seconds: number;
}

/**
 * What the instances of the benchmark need, and no instance: `bench/run.ts` launches them
 * from the launch templates, one per architecture, in the public subnets of an existing
 * VPC, and they terminate themselves.
 */
export class BenchStack extends Stack {
  constructor(scope: Construct, id: string, props: BenchStackProps) {
    super(scope, id, props);

    const vpc = ec2.Vpc.fromLookup(this, 'Vpc', props.vpcId ? { vpcId: props.vpcId } : { isDefault: true });
    // with a public address, the instances reach rustup and S3 without a NAT gateway
    if (vpc.publicSubnets.length === 0) {
      throw new Error(`${vpc.vpcId} has no public subnet`);
    }
    const securityGroup = new ec2.SecurityGroup(this, 'NoInbound', { vpc });

    const results = new s3.Bucket(this, 'Results', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      enforceSSL: true,
      removalPolicy: RemovalPolicy.DESTROY,
      autoDeleteObjects: true,
    });
    const source = new Asset(this, 'Source', { path: sourceArchive(props.ref) });

    const role = new iam.Role(this, 'Instance', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
      // Session Manager, to look inside an instance if a run goes wrong
      managedPolicies: [iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore')],
    });
    results.grantPut(role);
    // only the tarball, not the other CDK assets of the account
    role.addToPolicy(
      new iam.PolicyStatement({
        actions: ['s3:GetObject'],
        resources: [`arn:${this.partition}:s3:::${source.s3BucketName}/${source.s3ObjectKey}`],
      }),
    );

    const script = readFileSync(join(import.meta.dirname, '..', 'bench', 'instance.sh'), 'utf8')
      .replaceAll('@SOURCE@', source.s3ObjectUrl)
      .replaceAll('@RESULTS@', `s3://${results.bucketName}`)
      .replaceAll('@SECONDS@', String(props.seconds));

    const cpus = { Arm: ec2.AmazonLinuxCpuType.ARM_64, X86: ec2.AmazonLinuxCpuType.X86_64 };
    for (const [name, cpuType] of Object.entries(cpus)) {
      const template = new ec2.LaunchTemplate(this, `${name}Template`, {
        machineImage: ec2.MachineImage.latestAmazonLinux2023({ cpuType }),
        userData: ec2.UserData.custom(script),
        role,
        securityGroup,
        requireImdsv2: true,
        // the instance reads its Run tag
        instanceMetadataTags: true,
        spotOptions: {
          requestType: ec2.SpotRequestType.ONE_TIME,
          interruptionBehavior: ec2.SpotInstanceInterruption.TERMINATE,
        },
        instanceInitiatedShutdownBehavior: ec2.InstanceInitiatedShutdownBehavior.TERMINATE,
      });
      new CfnOutput(this, `${name}LaunchTemplate`, { value: template.launchTemplateId! });
    }

    new CfnOutput(this, 'Subnets', { value: vpc.publicSubnets.map((s) => s.subnetId).join(',') });
    new CfnOutput(this, 'ResultsBucket', { value: results.bucketName });
  }
}

/** The committed code of `ref`, as a tarball for the instances to build */
function sourceArchive(ref: string): string {
  const repo = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const dir = join(repo, 'infra', '.build');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `runner-chess-${ref}.tar.gz`);
  execFileSync('git', ['-C', repo, 'archive', '--format=tar.gz', '-o', file, ref]);
  return file;
}
