/** What the scripts run from here share: the stack's outputs, and the code they upload */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { CloudFormationClient, DescribeStacksCommand } from '@aws-sdk/client-cloudformation';
import { HeadObjectCommand, PutObjectCommand, type S3Client } from '@aws-sdk/client-s3';

export const STACK = 'RunnerChess';
export const repo = execFileSync('git', ['rev-parse', '--show-toplevel'], {
  encoding: 'utf8',
}).trim();

export async function stackOutputs(): Promise<Record<string, string>> {
  const response = await new CloudFormationClient({}).send(
    new DescribeStacksCommand({ StackName: STACK }),
  );
  const outputs = response.Stacks?.[0]?.Outputs ?? [];
  return Object.fromEntries(outputs.map((output) => [output.OutputKey!, output.OutputValue!]));
}

/**
 * Uploads the code of the engine as `sources/<hash>.tar.gz`, once, and gives its key.
 * `source` is a git ref, whose committed code goes, or `worktree`: every file of the
 * working tree that git doesn't ignore, committed, changed or new, as a network that
 * isn't committed yet.
 */
export async function uploadSource(s3: S3Client, bucket: string, source: string): Promise<string> {
  let tarball: Buffer;
  if (source === 'worktree') {
    const files = execFileSync('git', ['-C', repo, 'ls-files', '-co', '--exclude-standard', '-z'], {
      encoding: 'utf8',
    })
      .split('\0')
      .filter((file) => file && existsSync(join(repo, file)));
    tarball = execFileSync('tar', ['-czf', '-', '--no-xattrs', '-C', repo, '--null', '-T', '-'], {
      input: files.join('\0'),
      maxBuffer: 1 << 30,
      // without the ._ files of macOS
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    });
  } else {
    tarball = execFileSync('git', ['-C', repo, 'archive', '--format=tar.gz', source], {
      maxBuffer: 1 << 30,
    });
  }

  const key = `sources/${createHash('sha256').update(tarball).digest('hex').slice(0, 16)}.tar.gz`;
  try {
    await s3.send(new HeadObjectCommand({ Bucket: bucket, Key: key }));
  } catch {
    await s3.send(new PutObjectCommand({ Bucket: bucket, Key: key, Body: tarball }));
  }
  return key;
}
