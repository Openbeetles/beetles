import type { ChannelConnectivityItem } from "../api/endpoints/system";

const PASSIVE_CONNECTIVITY_UNAVAILABLE =
  "network.channel_connectivity_unavailable";

export function isChannelOperational(item: ChannelConnectivityItem): boolean {
  if (!item.configured) return false;
  return item.ok || item.runtime_status === "connected";
}

export function channelConnectivityMessageKey(
  item: ChannelConnectivityItem,
): string | null {
  const messageKey = item.message_key?.trim() || null;
  if (
    messageKey === PASSIVE_CONNECTIVITY_UNAVAILABLE &&
    isChannelOperational(item)
  ) {
    return null;
  }
  return messageKey;
}
