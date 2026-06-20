# firmups-backend Helm chart

Deploys the Firmups backend and a PostgreSQL cluster (managed by
[CloudNativePG](https://cloudnative-pg.io/)). It stores firmware in an
**external, S3-compatible object store** that you provide.

Two pieces of infrastructure must exist **before** installing this chart. They
are deliberately *not* deployed by it:

1. **CloudNativePG operator** — installed once per cluster; this chart only
   deploys a `Cluster` custom resource that the operator reconciles.
2. **S3-compatible object storage** — any S3-compatible store works
   (AWS S3, Ceph RGW, Garage, …). This chart only holds the connection
   details and credentials the backend uses to reach it.

## Prerequisite 1: CloudNativePG operator

Install the operator **once per cluster**. Pick whichever method matches your
environment:

```sh
# Option A — Helm (any cluster)
helm repo add cnpg https://cloudnative-pg.io/charts && helm repo update
helm upgrade --install cnpg cnpg/cloudnative-pg \
  --version 0.28.3 --namespace cnpg-system --create-namespace --wait

# Option B — plain manifest (any cluster)
kubectl apply --server-side -f \
  https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/release-1.27/releases/cnpg-1.27.0.yaml

# Option C — MicroK8s addon
microk8s enable cloudnative-pg
```

Verify before continuing:

```sh
kubectl get deploy -n cnpg-system           # operator pod should be Ready
kubectl get crd clusters.postgresql.cnpg.io
```

> The operator's CRDs are cluster-wide. Deleting them cascade-deletes **every**
> CNPG cluster on the machine, so remove them only when decommissioning the
> whole cluster.

## Prerequisite 2: S3-compatible object storage

Provide an S3-compatible bucket and an access key/secret with read+write access
to it. This can be a managed service (e.g. AWS S3) or a self-hosted store you
run separately. How you provision it is up to you — this chart does not manage
it.

Once the bucket and credentials exist, point the chart at them via the
`storage` values:

| Value                  | Description                                              |
| ---------------------- | -------------------------------------------------------- |
| `storage.s3Endpoint`   | S3 API endpoint URL (e.g. `https://s3.amazonaws.com`)    |
| `storage.region`       | S3 region (must match the bucket's region)               |
| `storage.bucket`       | Bucket name firmware is stored in                        |
| `storage.keyId`        | S3 access key ID                                         |
| `storage.accessKey`    | S3 secret access key                                     |

Set `keyId` and `accessKey` via a secrets override file (`-f`), never in a
committed values file.

## Install the chart

```sh
helm upgrade --install firmups ./helm \
  --namespace firmups --create-namespace --wait
```

Provide credentials and overrides via values files (`-f`). At minimum set:
`app.apiKey`, `postgresql.password`, `storage.s3Endpoint`, `storage.region`,
`storage.bucket`, `storage.keyId`, and `storage.accessKey`.

## MicroK8s notes

MicroK8s bundles its own Helm and kubectl — prefix commands with `microk8s`
(`microk8s helm3 ...`, `microk8s kubectl ...`), or alias them. The
`hostpath-storage` and `metrics-server` addons provide the default StorageClass
and HPA metrics this chart relies on. To use a locally built image, push it to
the built-in registry (`registry` addon, `localhost:32000`) and point
`image.repository` at it.
