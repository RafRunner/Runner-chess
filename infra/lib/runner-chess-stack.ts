import { CfnOutput, Duration, RemovalPolicy, Stack, type StackProps, Tags } from 'aws-cdk-lib';
import * as autoscaling from 'aws-cdk-lib/aws-autoscaling';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as s3 from 'aws-cdk-lib/aws-s3';
import type { Construct } from 'constructs';
import { gzippedUserData, lookupVpc, userData } from './stack-helpers.ts';

export interface RunnerChessStackProps extends StackProps {
  /** The VPC the instances run in, the default one of the region without it */
  vpcId?: string;
  /** The Spot types a worker can be, all arm64 */
  workerTypes: string[];
  /** The most workers at once */
  maxWorkers: number;
}

/**
 * Everything of runner-chess on AWS, and nothing that costs while idle:
 *
 * - a bucket for all of it, which outlives the stack: the sources the instances build,
 *   openings, the job, the data of datagen, the training data prepared from it, the state
 *   of SPRTs and the benchmarks;
 * - an empty group of Spot workers, that play the job of `job.json` when
 *   `worker/run.ts` asks for them, and empty the group again when it ends, by its
 *   deadline at the latest, or when one of them fails;
 * - a launch template per architecture for the benchmark of instances, which
 *   `bench/run.ts` launches one by one, and which terminate themselves.
 */
export class RunnerChessStack extends Stack {
  constructor(scope: Construct, id: string, props: RunnerChessStackProps) {
    super(scope, id, props);

    const vpc = lookupVpc(this, props.vpcId);
    const securityGroup = new ec2.SecurityGroup(this, 'NoInbound', { vpc });
    const bucket = new s3.Bucket(this, 'Data', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      enforceSSL: true,
      removalPolicy: RemovalPolicy.RETAIN,
      lifecycleRules: [
        // training data, which a prepare job makes again from datagen/ in minutes
        { prefix: 'prepared/', expiration: Duration.days(30) },
        { abortIncompleteMultipartUploadAfter: Duration.days(1) },
      ],
    });
    // Session Manager, to look inside an instance
    const ssm = iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore');

    const workerRole = new iam.Role(this, 'Worker', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
      managedPolicies: [ssm],
    });
    bucket.grantRead(workerRole);
    bucket.grantPut(workerRole);
    workerRole.addToPolicy(
      new iam.PolicyStatement({
        actions: ['autoscaling:DescribeAutoScalingInstances'],
        resources: ['*'],
      }),
    );

    const workerTemplate = new ec2.LaunchTemplate(this, 'WorkerTemplate', {
      machineImage: ec2.MachineImage.latestAmazonLinux2023({
        cpuType: ec2.AmazonLinuxCpuType.ARM_64,
      }),
      role: workerRole,
      securityGroup,
      requireImdsv2: true,
      // the worker reads its Bucket tag
      instanceMetadataTags: true,
      instanceInitiatedShutdownBehavior: ec2.InstanceInitiatedShutdownBehavior.TERMINATE,
      // a prepare job holds its data twice, 20 GB for 325 million positions
      blockDevices: [
        {
          deviceName: '/dev/xvda',
          volume: ec2.BlockDeviceVolume.ebs(64, {
            volumeType: ec2.EbsDeviceVolumeType.GP3,
            deleteOnTermination: true,
          }),
        },
      ],
    });

    // gzipped, and so without the bucket's name in it, which a tag gives instead
    (workerTemplate.node.defaultChild as ec2.CfnLaunchTemplate).addPropertyOverride(
      'LaunchTemplateData.UserData',
      gzippedUserData('worker/worker.py'),
    );

    // no desired capacity here, or every deploy would reset it
    const workers = new autoscaling.AutoScalingGroup(this, 'Workers', {
      vpc,
      vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
      minCapacity: 0,
      maxCapacity: props.maxWorkers,
      mixedInstancesPolicy: {
        launchTemplate: workerTemplate,
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
    Tags.of(workers).add('Bucket', bucket.bucketName);
    // a policy of its own: in the role's default one, the group would depend on itself
    new iam.Policy(this, 'LeaveTheGroup', {
      roles: [workerRole],
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

    const benchRole = new iam.Role(this, 'Bench', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
      managedPolicies: [ssm],
    });
    bucket.grantRead(benchRole, 'sources/*');
    bucket.grantPut(benchRole, 'bench/*');
    const cpus = { Arm: ec2.AmazonLinuxCpuType.ARM_64, X86: ec2.AmazonLinuxCpuType.X86_64 };
    for (const [name, cpuType] of Object.entries(cpus)) {
      const template = new ec2.LaunchTemplate(this, `Bench${name}Template`, {
        machineImage: ec2.MachineImage.latestAmazonLinux2023({ cpuType }),
        userData: userData('bench/instance.py', { BUCKET: bucket.bucketName }),
        role: benchRole,
        securityGroup,
        requireImdsv2: true,
        // the instance reads its Run and Source tags
        instanceMetadataTags: true,
        spotOptions: {
          requestType: ec2.SpotRequestType.ONE_TIME,
          interruptionBehavior: ec2.SpotInstanceInterruption.TERMINATE,
        },
        instanceInitiatedShutdownBehavior: ec2.InstanceInitiatedShutdownBehavior.TERMINATE,
      });
      new CfnOutput(this, `Bench${name}LaunchTemplate`, { value: template.launchTemplateId! });
    }

    new CfnOutput(this, 'Bucket', { value: bucket.bucketName });
    new CfnOutput(this, 'WorkersGroup', { value: workers.autoScalingGroupName });
    new CfnOutput(this, 'Subnets', { value: vpc.publicSubnets.map((s) => s.subnetId).join(',') });
  }
}
