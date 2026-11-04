import { CfnOutput, RemovalPolicy, Stack, type StackProps } from 'aws-cdk-lib';
import * as autoscaling from 'aws-cdk-lib/aws-autoscaling';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as s3 from 'aws-cdk-lib/aws-s3';
import { Asset } from 'aws-cdk-lib/aws-s3-assets';
import type { Construct } from 'constructs';
import { lookupVpc, sourceArchive, userData } from './source.ts';

export interface DatagenStackProps extends StackProps {
  /** The VPC the workers run in, the default one of the region without it */
  vpcId?: string;
  /** The git ref of the engine that plays, as `git archive` takes it */
  ref: string;
  /** `bench search` of `ref`: a worker that counts other nodes would play other games */
  bench: number;
  /** The Spot types a worker can be, all arm64 */
  workerTypes: string[];
  /** The most workers at once */
  maxWorkers: number;
}

/**
 * Spot workers that play the job of `job.json` in the bucket, chunk after chunk, until
 * every part of it has its positions. The group is empty until `datagen/run.ts start`
 * asks for workers, and empties itself again when the job ends, by its deadline at
 * the latest, or when a worker fails.
 */
export class DatagenStack extends Stack {
  constructor(scope: Construct, id: string, props: DatagenStackProps) {
    super(scope, id, props);

    const vpc = lookupVpc(this, props.vpcId);
    const securityGroup = new ec2.SecurityGroup(this, 'NoInbound', { vpc });

    // the data outlives the stack
    const data = new s3.Bucket(this, 'Data', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      enforceSSL: true,
      removalPolicy: RemovalPolicy.RETAIN,
    });
    const source = new Asset(this, 'Source', { path: sourceArchive(props.ref) });

    const role = new iam.Role(this, 'Worker', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
      // Session Manager, to look inside a worker
      managedPolicies: [iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore')],
    });
    data.grantRead(role);
    data.grantPut(role);
    role.addToPolicy(
      new iam.PolicyStatement({
        actions: ['s3:GetObject'],
        resources: [`arn:${this.partition}:s3:::${source.s3BucketName}/${source.s3ObjectKey}`],
      }),
    );
    role.addToPolicy(
      new iam.PolicyStatement({
        actions: ['autoscaling:DescribeAutoScalingInstances'],
        resources: ['*'],
      }),
    );

    const template = new ec2.LaunchTemplate(this, 'Template', {
      machineImage: ec2.MachineImage.latestAmazonLinux2023({
        cpuType: ec2.AmazonLinuxCpuType.ARM_64,
      }),
      userData: userData('datagen/worker.py', {
        SOURCE: source.s3ObjectUrl,
        BUCKET: data.bucketName,
        BENCH: String(props.bench),
      }),
      role,
      securityGroup,
      requireImdsv2: true,
      instanceInitiatedShutdownBehavior: ec2.InstanceInitiatedShutdownBehavior.TERMINATE,
    });

    // no desired capacity here, or every deploy would reset it
    const workers = new autoscaling.AutoScalingGroup(this, 'Workers', {
      vpc,
      vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
      minCapacity: 0,
      maxCapacity: props.maxWorkers,
      mixedInstancesPolicy: {
        launchTemplate: template,
        launchTemplateOverrides: props.workerTypes.map((type) => ({
          instanceType: new ec2.InstanceType(type),
        })),
        instancesDistribution: {
          onDemandBaseCapacity: 0,
          onDemandPercentageAboveBaseCapacity: 0,
          spotAllocationStrategy: autoscaling.SpotAllocationStrategy.PRICE_CAPACITY_OPTIMIZED,
        },
      },
    });
    // a policy of its own: in the role's default one, the group would depend on itself
    new iam.Policy(this, 'LeaveTheGroup', {
      roles: [role],
      statements: [
        new iam.PolicyStatement({
          actions: [
            'autoscaling:SetDesiredCapacity',
            'autoscaling:TerminateInstanceInAutoScalingGroup',
          ],
          resources: [workers.autoScalingGroupArn],
        }),
      ],
    });

    new CfnOutput(this, 'DataBucket', { value: data.bucketName });
    new CfnOutput(this, 'WorkersGroup', { value: workers.autoScalingGroupName });
  }
}
