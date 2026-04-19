import { useCallback } from "react";
import { useConfigPageLoad } from "./useConfigPageLoad.ts";
import { useSaveFeedback } from "./useSaveFeedback.ts";
import { useUnsaved } from "./useUnsaved.ts";
import {
  runValidatedConfigSave,
  type ConfigSaveResult,
} from "./configSaveLifecycle.ts";

export interface UseConfigEditorControllerOptions {
  t: (key: string) => string;
  hasData: boolean;
  loading: boolean;
  load: () => Promise<void>;
  canLoad?: boolean;
}

export interface RunConfigEditorSaveOptions<TResult extends ConfigSaveResult> {
  validate: () => string | null;
  performSave: () => Promise<TResult>;
  onBeforeSave?: () => void;
  onSuccess?: (result: TResult) => void;
}

export function useConfigEditorController({
  t,
  hasData,
  loading,
  load,
  canLoad = true,
}: UseConfigEditorControllerOptions) {
  const saveFeedback = useSaveFeedback(t);
  const { setDirty } = useUnsaved();

  useConfigPageLoad({
    hasConfig: hasData,
    loading,
    loadConfig: load,
    canLoad,
  });

  const markDirty = useCallback(() => {
    setDirty(true);
  }, [setDirty]);

  const markClean = useCallback(() => {
    setDirty(false);
  }, [setDirty]);

  const runSave = useCallback(
    async <TResult extends ConfigSaveResult>({
      validate,
      performSave,
      onBeforeSave,
      onSuccess,
    }: RunConfigEditorSaveOptions<TResult>) =>
      runValidatedConfigSave({
        validate,
        saveFeedback,
        performSave,
        markClean,
        onBeforeSave,
        onSuccess,
      }),
    [markClean, saveFeedback],
  );

  return {
    saveFeedback,
    saveDisabled: saveFeedback.status === "saving",
    markDirty,
    markClean,
    runSave,
  };
}
