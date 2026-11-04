import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import type { Construct } from 'constructs';

const infra = join(import.meta.dirname, '..');

/** The committed code of `ref`, as a tarball for the instances to build */
export function sourceArchive(ref: string): string {
  const repo = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const dir = join(infra, '.build');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `runner-chess-${ref}.tar.gz`);
  execFileSync('git', ['-C', repo, 'archive', '--format=tar.gz', '-o', file, ref]);
  return file;
}

/** A script of infra/ as user data, with every `@KEY@` replaced */
export function userData(path: string, values: Record<string, string>): ec2.UserData {
  let script = readFileSync(join(infra, path), 'utf8');
  for (const [key, value] of Object.entries(values)) {
    script = script.replaceAll(`@${key}@`, value);
  }
  return ec2.UserData.custom(script);
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
