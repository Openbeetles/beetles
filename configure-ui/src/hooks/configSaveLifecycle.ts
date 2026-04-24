export interface ConfigSaveResult {
  ok: boolean;
  error?: string;
  restartRequired?: boolean;
}

export const CONFIG_SAVE_IN_PROGRESS_ERROR = "config.saveInProgress";

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

export interface ConfigSaveSingleFlightSlot<TResult extends ConfigSaveResult> {
  current: Promise<TResult> | null;
}

export function runSingleFlightConfigSave<TResult extends ConfigSaveResult>(
  slot: ConfigSaveSingleFlightSlot<TResult>,
  run: () => Promise<TResult>,
): Promise<TResult> {
  if (slot.current) {
    return Promise.resolve({
      ok: false,
      error: CONFIG_SAVE_IN_PROGRESS_ERROR,
    } as TResult);
  }
  const task = run().finally(() => {
    if (slot.current === task) {
      slot.current = null;
    }
  });
  slot.current = task;
  return task;
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
