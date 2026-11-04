# infra

AWS for runner-chess, with the CDK in TypeScript. A project of its own, so the engine
stays without dependencies.

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
npx cdk bootstrap          # once per account and region
npx cdk deploy             # -c vpcId=<id>, -c ref=<tag>, -c seconds=<n>
npm run bench              # or: npm run bench -- c7g.4xlarge c7a.4xlarge
npx cdk destroy            # deletes the results too
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
