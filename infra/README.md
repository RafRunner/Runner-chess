# infra

AWS for runner-chess, with the CDK in TypeScript. A project of its own, so the engine
stays without dependencies.

`RunnerChess` is the one stack, and holds nothing that costs while idle:

- a bucket for all of it, which `cdk destroy` keeps: `sources/` the engines the
  instances build, `books/`, `job.json`, `datagen/`, `prepared/`, `sprt/`, `bench/` and
  `logs/`;
- an Auto Scaling group of Spot workers, c8g, m8g, c7g or m7g.4xlarge, where the m ones
  are the same Graviton4 and 3 with more memory, 4 at most for a Spot quota of 64 vCPUs,
  empty until a job asks for them;
- a launch template per architecture for the benchmark of instances.

The instances run Python, for the Python 3.9 of Amazon Linux 2023, as their whole user
data: `worker/worker.py` and `bench/instance.py`. Each installs boto3 from the image's
packages, and leaves with the AWS CLI of the image, which works even when boto3 didn't
install. They build the engine for their own CPU, from a tarball that the scripts here
upload to `sources/`: the committed code of a git ref, or the working tree as it is,
`worktree`, a network not committed yet included. A new version needs no deploy.

User data has 16 KB at most, which `worker/worker.py` is past: the stack gives it
gzipped, which cloud-init decompresses, and so without the bucket's name written in it,
the worker reads its `Bucket` tag instead. A worker has a disk of 64 GB, for the data a
prepare job holds twice.

```
npm install
npx cdk bootstrap               # once per account and region
npx cdk deploy RunnerChess      # -c vpcId=<id>, -c maxWorkers=<n>, -c workerTypes=...
```

The instances run in the public subnets of an existing VPC, the default one of the
region unless `-c vpcId=<id>` says another, so they reach rustup and S3 without a NAT
gateway. The region is the one of the AWS profile, or `AWS_REGION`.

## Jobs

The workers play one job at a time, `job.json` in the bucket, which they read again as
they go: `stop`, or the deadline that every job gets, 24 hours by default, empties the
group, and so does a worker that fails, since the one that would replace it would fail
the same way. Every worker's log goes to `logs/<instance>.log`. A job doesn't begin
while workers still play another, unless `--force`.

```
npm run job -- datagen worker/jobs/smoke.json --workers 1 --hours 1 --name smoke-2
npm run job -- sprt worktree 1.7.0 --engine-name net-010-c
npm run job -- prepare net-011 1.8.0 1.7.0 1.5.0
npm run job -- upload 1.5.0 ../data/1.5.0-random ../data/1.5.0-book
npm run job -- status
npm run job -- fetch                    # the current job, or: fetch <job>
npm run job -- stop
```

### datagen

A job file, as in `worker/jobs/`, has the source of the engine and the node count of
its `bench search`, which every worker checks, the size of a chunk, and parts, each
with its positions, its nodes a move and maybe a book, a path from the repository. Every
worker plays, chunk after chunk, the first part that the bucket doesn't have all the
positions of yet, and uploads the chunk as
`datagen/<job>/<part>/<seed>-<positions>.txt.zst`:

- a worker reclaimed by Spot loses only the chunk it was playing, about a minute and a
  half of 250,000 positions, and the group replaces it with another;
- the seed comes from the instance and the chunk, so no two chunks play the same games;
- when every part has its positions, each worker leaves the group, one fewer each time.

`fetch` downloads the chunks into `../data/<job>-<part>/`, as the text of datagen, and
resumes. A job whose positions the bucket already has doesn't start: `--name` plays the
same file as another job.

The smoke job ran on 2026-11-04 on a c7g.4xlarge: a chunk of 250,000 positions every 100
seconds, the first one 3 minutes after the start, building included, and 14.5 bytes a
position compressed. Spot reclaimed the worker in the middle of the chunk of the attacks,
and the group launched another 2 seconds later, which built the engine again, played
that chunk and left: 1,259,936 positions, none malformed.

### prepare

The training data of `../trainer/`, on one worker, as `../scripts/prepare-data.sh`
makes it here: the chunks of the datagen jobs given, or of parts of them as
`<job>/<part>`, each converted to bullet's binary format by the `bullet-utils` of the
commit `../trainer/Cargo.toml` names, put together and shuffled in memory, into
`prepared/<name>.data`, with `prepared/<name>.json` telling how many positions it has.
It takes twice the data on disk and 4/3 of it in memory, which a worker of 32 GB has for
up to 750 million positions. A worker reclaimed in the middle loses all of it, and the
one that replaces it starts over. The deadline is 3 hours, unless `--hours`.

`fetch <name>` downloads it into `../data/<name>.data`, and resumes: only the data the
trainer reads comes here, 32 bytes a position, and not the text it was made from, twice
as big, nor the copy of it that shuffling takes.

`upload <job> <directories>...` stores datagen played here, the `.txt` files of
`../data/<job>-<part>/` directories, as the chunks of the datagen job `<job>`, for
`prepare` to take as any other. It skips the files already there.

The smoke job, prepared on 2026-11-04 on a c7g.4xlarge, took a minute from the start,
`bullet-utils` built in 9 seconds: the same 1,259,936 positions as converting their text
here at once, in another order.

### sprt

A match of fastchess on one worker, as `scripts/match.sh --sprt` plays here: the engine
against the baseline, git refs or `worktree`, from `8moves_v3.pgn`, until the SPRT
[0, 10] decides, or `--bounds`. The worker builds both engines and fastchess 1.8.2, the
version of the repository's SPRTs, and plays as many games at once as it has vCPUs but
one. The time control, 10+0.1 or `--tc`, is the one of this computer: the worker scales
it by how many fewer nodes a second the baseline searches there than here, measured by
`bench search 17` of `engines/runner-chess-<baseline>`, or given as `--reference-nps`.

The state of fastchess, its games and its output go to `sprt/<job>/` every minute, and
fastchess saves its state every 10 games: the worker that replaces a reclaimed one
resumes the match from there, with the time control it started with. `status` shows
the last results, and `fetch` brings the games and the output to
`../engines/games/<job>.pgn` and `.log`.

## The benchmark of instances

How many positions a second `datagen` makes on each kind of instance, and what 100
million would cost at the current Spot price. `bench/run.ts` launches a Spot instance of
each type, outside the group, in the zone where its type is cheapest, and each one
builds the engine, runs `bench search`, whose node count must be the same on every CPU,
plays `datagen` on all its vCPUs for 300 seconds, leaves the results under `bench/` and
terminates itself, after 45 minutes at most whatever happens.

```
npm run bench                   # or: npm run bench -- --source 1.7.0 c7g.4xlarge c7a.4xlarge
```

The default types, five of 4 vCPUs, take 20 vCPUs of the Spot quota at once ("All
Standard ... Spot Instance Requests" in Service Quotas, 32 on an account that never
asked for more). The table goes to the console, and every run to
`.build/bench-<run>.json`.

### Results

1.7.0 in us-west-2d on 2026-11-04, 290 seconds of `datagen` on 4 vCPUs, each built for
its own CPU. Every one searched the 22792 nodes of `bench search`, as on the M4, whose
cores make 345 positions a second each.

| type | CPU | positions/s | per vCPU | Spot $/h | $ per 100M |
|---|---|---|---|---|---|
| c6g.xlarge | Graviton2 | 462 | 116 | 0.0591 | 3.55 |
| c7g.xlarge | Graviton3 | 616 | 154 | 0.0473 | 2.13 |
| c8g.xlarge | Graviton4 | 774 | 194 | 0.0593 | 2.13 |
| c7a.xlarge | EPYC 9R14, Zen 4 | 877 | 219 | 0.0898 | 2.84 |
| c7i.xlarge | Sapphire Rapids | no Spot capacity | | | |

Graviton3 and 4 cost the same for the positions, and Graviton4 makes them in 20% less
time. Zen 4 is the fastest core, by 13% over Graviton4, for 51% more an hour.

