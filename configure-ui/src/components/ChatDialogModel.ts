export type ChatDialogCloseReason =
  | "explicit"
  | "backdropClick"
  | "escapeKeyDown";

export function shouldCloseChatDialog(reason: ChatDialogCloseReason): boolean {
  return reason === "explicit";
}
