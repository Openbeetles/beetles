export interface ConfigSaveResult {
  ok: boolean;
  error?: string;
  restartRequired?: boolean;
}

export interface ConfigSaveFeedbackDriver {
  begin: () => void;
  fail: (message: string) => void;
  finishFromResult: (result: ConfigSaveResult) => void;
}

export interface RunValidatedConfigSaveOptions<TResult extends ConfigSaveResult> {
  validate: () => string | null;
  saveFeedback: ConfigSaveFeedbackDriver;
  performSave: () => Promise<TResult>;
  markClean?: () => void;
  onBeforeSave?: () => void;
  onSuccess?: (result: TResult) => void;
}

export async function runValidatedConfigSave<TResult extends ConfigSaveResult>({
  validate,
  saveFeedback,
  performSave,
  markClean,
  onBeforeSave,
  onSuccess,
}: RunValidatedConfigSaveOptions<TResult>): Promise<TResult | null> {
  const error = validate();
  if (error) {
    saveFeedback.fail(error);
    return null;
  }
  saveFeedback.begin();
  onBeforeSave?.();
  const result = await performSave();
  saveFeedback.finishFromResult(result);
  if (result.ok) {
    markClean?.();
    onSuccess?.(result);
  }
  return result;
}
