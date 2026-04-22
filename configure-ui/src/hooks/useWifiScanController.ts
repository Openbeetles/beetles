import { useCallback, useState } from "react";
import type { TFunction } from "i18next";
import type { ApiResult } from "../api/client";
import type { WifiApEntry } from "../api/endpoints/system";
import { translateApiError } from "../i18n/apiErrors";

interface UseWifiScanControllerArgs {
  canScan: boolean;
  scan: () => Promise<ApiResult<WifiApEntry[]>>;
  t: TFunction;
}

export function useWifiScanController({
  canScan,
  scan,
  t,
}: UseWifiScanControllerArgs) {
  const [wifiScanList, setWifiScanList] = useState<WifiApEntry[] | null>(null);
  const [wifiScanLoading, setWifiScanLoading] = useState(false);
  const [wifiScanError, setWifiScanError] = useState("");

  const handleWifiScan = useCallback(async () => {
    if (!canScan) return;
    setWifiScanLoading(true);
    setWifiScanList(null);
    setWifiScanError("");
    const res = await scan();
    setWifiScanLoading(false);
    if (res.ok && Array.isArray(res.data)) {
      setWifiScanList(res.data);
      setWifiScanError("");
      return;
    }
    setWifiScanList([]);
    setWifiScanError(translateApiError(t, res.error, "config.wifiScanFailed"));
  }, [canScan, scan, t]);

  const resetWifiScan = useCallback(() => {
    setWifiScanList(null);
    setWifiScanLoading(false);
    setWifiScanError("");
  }, []);

  return {
    wifiScanList,
    wifiScanLoading,
    wifiScanError,
    handleWifiScan,
    resetWifiScan,
  };
}
