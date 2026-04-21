export interface SplitPageErrorStateOptions {
  hasData: boolean;
  loading: boolean;
  error?: string | null;
  suppress?: boolean;
}

export interface SplitPageErrorStateResult {
  inlineError: string | null;
  blockingError: string | null;
}

function normalizeErrorMessage(message: string | null | undefined): string | null {
  const trimmed = message?.trim();
  return trimmed ? trimmed : null;
}

/**
 * 页面级加载反馈分流：
 * - 有内容时，错误仅作为顶部轻提示（inlineError）
 * - 无内容时，错误作为阻塞态（blockingError）
 * - loading / suppress 时不展示错误
 */
export function splitPageErrorState({
  hasData,
  loading,
  error,
  suppress = false,
}: SplitPageErrorStateOptions): SplitPageErrorStateResult {
  const normalized = normalizeErrorMessage(error);
  if (!normalized || loading || suppress) {
    return { inlineError: null, blockingError: null };
  }
  if (hasData) {
    return { inlineError: normalized, blockingError: null };
  }
  return { inlineError: null, blockingError: normalized };
}
