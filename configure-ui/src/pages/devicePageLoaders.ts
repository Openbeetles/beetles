import type { ApiResult } from "../api/client.ts";
import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
} from "../api/endpoints/system.ts";

export interface DeviceStatusBundleData {
  health: HealthData;
  resource: ResourceSnapshotData;
  metrics: MetricsSnapshotData;
}

export type DeviceStatusBundleResult =
  | { ok: true; data: DeviceStatusBundleData }
  | { ok: false; error: string };

export interface DeviceStatusLoaders {
  health: () => Promise<ApiResult<HealthData>>;
  resource: () => Promise<ApiResult<ResourceSnapshotData>>;
  metrics: () => Promise<ApiResult<MetricsSnapshotData>>;
}

export async function loadDeviceStatusBundle(
  loaders: DeviceStatusLoaders,
): Promise<DeviceStatusBundleResult> {
  try {
    const healthRes = await loaders.health();
    if (!healthRes.ok || !healthRes.data) {
      return { ok: false, error: healthRes.error ?? "" };
    }

    const resourceRes = await loaders.resource().catch(() => null);
    const metricsRes = await loaders.metrics().catch(() => null);

    return {
      ok: true,
      data: {
        health: healthRes.data,
        resource:
          resourceRes && resourceRes.ok && resourceRes.data ? resourceRes.data : {},
        metrics: metricsRes && metricsRes.ok && metricsRes.data ? metricsRes.data : {},
      },
    };
  } catch {
    return {
      ok: false,
      error: "config.errorNetwork",
    };
  }
}
