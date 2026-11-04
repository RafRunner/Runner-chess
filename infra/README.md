# infra

AWS for runner-chess, with the CDK in TypeScript. A project of its own, so the engine
stays without dependencies.

What runs on the instances is Python, for the Python 3.9 of Amazon Linux 2023, and is
their whole user data: `bench/instance.py` and `datagen/worker.py`. Each installs boto3
from the image's packages, and leaves with the AWS CLI of the image, which works even
when boto3 didn't install.

## The benchmark of instances

How many positions a second `datagen` makes on each kind of instance, and what 100
million would cost at the current Spot price. `RunnerChessBench` holds what the
instances need, and nothing that costs while idle: a security group, a bucket for the
results, a role, and a launch template per architecture. The instances run in the
public subnets of an existing VPC, the default one of the region unless `-c
vpcId=<id>` says another, so they reach rustup and S3 without a NAT gateway.
`bench/run.ts` launches them, a Spot instance of each type, and each one:

1. builds the committed code of a tag, 1.7.0 by default, with `-C target-cpu=native`;
2. runs `bench search`, whose node count must be the same as on every other CPU;
3. plays `datagen` on all its vCPUs for 300 seconds, from random openings;
4. leaves the results in the bucket and terminates itself, after 45 minutes at most
   whatever happens.

```
npm install
npx cdk bootstrap                 # once per account and region
npx cdk deploy RunnerChessBench   # -c vpcId=<id>, -c ref=<tag>, -c seconds=<n>
npm run bench                     # or: npm run bench -- c7g.4xlarge c7a.4xlarge
npx cdk destroy RunnerChessBench  # deletes the results too
```

The region is the one of the AWS profile, or `AWS_REGION`: us-east-1, us-east-2 and
us-west-2 have every type and the lowest Spot prices, and each instance goes to the zone
where its type is cheapest. The default types, five of 4 vCPUs, take 20 vCPUs of the
Spot quota at once ("All Standard ... Spot Instance Requests" in Service Quotas, 32 on
an account that never asked for more). The table goes to the console, and every run to
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

## The generation of data

`RunnerChessDatagen` is an Auto Scaling group of Spot workers, c8g.4xlarge or
c7g.4xlarge, 2 at most for the Spot quota of 32 vCPUs, empty until a job asks for them,
and a bucket for the data, which `cdk destroy` keeps. A job, as in `datagen/jobs/`, has
parts, each with its positions, its nodes a move and maybe a book, and the size of a
chunk. Every worker builds the engine for its own CPU, checks the node count of `bench
search`, and then, chunk after chunk, plays the first part that the bucket doesn't have
all the positions of yet, and uploads the chunk as `<job>/<part>/<seed>-<positions>.txt.zst`:

- a worker reclaimed by Spot loses only the chunk it was playing, about a minute and a
  half of 250,000 positions, and the group replaces it with another;
- the seed comes from the instance and the chunk, so no two chunks play the same games;
- when every part has its positions, each worker leaves the group, one fewer each time;
- the job is read again before every chunk: `stop`, or the deadline that `start` sets,
  empties the group at the next one, and so does a worker that fails, whose log is in
  `logs/<instance>.log` as every worker's.

```
npx cdk deploy RunnerChessDatagen     # -c maxWorkers=<n> once the quota allows more
npm run datagen -- start datagen/jobs/smoke.json --workers 1 --hours 1 --name smoke-2
npm run datagen -- status             # positions by part, rate, workers
npm run datagen -- fetch              # into ../data/<job>-<part>/, resumes
npm run datagen -- stop
```

The workers play the tag of `-c ref`, and `-c bench` is its node count. The books of a
job are paths from the repository, uploaded by `start`.

The smoke job ran on 2026-11-04 on a c7g.4xlarge: a chunk of 250,000 positions every 100
seconds, the first one 3 minutes after the start, building included, and 14.5 bytes a
position compressed. Spot reclaimed the worker in the middle of the chunk of the attacks,
and the group launched another 2 seconds later, which built the engine again, played
that chunk and left: 1,259,936 positions, none malformed.
