import type { ApiResult } from "../api/client.ts";
import type {
  ChannelConnectivityItem,
  ChannelConnectivityResponse,
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
} from "../api/endpoints/system.ts";

export interface DeviceHealthBundleData {
  health: HealthData;
  resource: ResourceSnapshotData;
  metrics: MetricsSnapshotData;
}

export type DeviceHealthBundleResult =
  | { ok: true; data: DeviceHealthBundleData }
  | { ok: false; error: string };

export type DeviceChannelConnectivityResult =
  | { ok: true; data: ChannelConnectivityItem[] }
  | { ok: false; error: string };

export interface DeviceHealthLoaders {
  health: () => Promise<ApiResult<HealthData>>;
  resource: () => Promise<ApiResult<ResourceSnapshotData>>;
  metrics: () => Promise<ApiResult<MetricsSnapshotData>>;
}

export async function loadDeviceHealthBundle(
  loaders: DeviceHealthLoaders,
): Promise<DeviceHealthBundleResult> {
  try {
    const healthRes = await loaders.health();
    if (!healthRes.ok || !healthRes.data) {
      return { ok: false, error: healthRes.error ?? "" };
    }

    const resourceRes = await loaders.resource();
    if (!resourceRes.ok || !resourceRes.data) {
      return { ok: false, error: resourceRes.error ?? "" };
    }

    const metricsRes = await loaders.metrics();
    if (!metricsRes.ok || !metricsRes.data) {
      return { ok: false, error: metricsRes.error ?? "" };
    }

    return {
      ok: true,
      data: {
        health: healthRes.data,
        resource: resourceRes.data,
        metrics: metricsRes.data,
      },
    };
  } catch {
    return {
      ok: false,
      error: "config.errorNetwork",
    };
  }
}

export async function loadDeviceChannelConnectivity(
  load: () => Promise<ApiResult<ChannelConnectivityResponse>>,
): Promise<DeviceChannelConnectivityResult> {
  try {
    const result = await load();
    if (result.ok && result.data?.channels) {
      return {
        ok: true,
        data: result.data.channels,
      };
    }
    return {
      ok: false,
      error: result.error ?? "network.channel_connectivity_unavailable",
    };
  } catch {
    return {
      ok: false,
      error: "config.errorNetwork",
    };
  }
}
