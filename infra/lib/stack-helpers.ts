import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import type { Construct } from 'constructs';

const infra = join(import.meta.dirname, '..');

/** A script of infra/ as user data, with every `@KEY@` replaced */
export function userData(path: string, values: Record<string, string>): ec2.UserData {
  let script = readFileSync(join(infra, path), 'utf8');
  for (const [key, value] of Object.entries(values)) {
    script = script.replaceAll(`@${key}@`, value);
  }
  return ec2.UserData.custom(script);
}

/**
 * A script of infra/ gzipped, in base64, as the user data of a launch template takes it:
 * cloud-init runs it decompressed, and so a script past the 16 KB of user data fits
 */
export function gzippedUserData(path: string): string {
  return gzipSync(readFileSync(join(infra, path)), { level: 9 }).toString('base64');
}

/** The VPC to run in, the default one of the region without `vpcId` */
export function lookupVpc(scope: Construct, vpcId?: string): ec2.IVpc {
  const vpc = ec2.Vpc.fromLookup(scope, 'Vpc', vpcId ? { vpcId } : { isDefault: true });
  // with a public address, the instances reach rustup and S3 without a NAT gateway
  if (vpc.publicSubnets.length === 0) {
    throw new Error(`${vpc.vpcId} has no public subnet`);
  }
  return vpc;
}
