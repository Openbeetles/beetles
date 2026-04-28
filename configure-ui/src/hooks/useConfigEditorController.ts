import { useCallback, useEffect, useId } from "react";
import type { ApiErrorTranslator } from "../i18n/apiErrors.ts";
import { useConfigPageLoad } from "./useConfigPageLoad.ts";
import { useSaveFeedback } from "./useSaveFeedback.ts";
import { useUnsaved } from "./useUnsaved.ts";
import {
  runValidatedConfigSave,
  type ConfigSaveResult,
} from "./configSaveLifecycle.ts";

export interface UseConfigEditorControllerOptions {
  t: ApiErrorTranslator;
  hasData: boolean;
  loading: boolean;
  load: () => Promise<void>;
  canLoad?: boolean;
  dirtyOwner?: string;
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
  dirtyOwner,
}: UseConfigEditorControllerOptions) {
  const saveFeedback = useSaveFeedback(t);
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const generatedOwner = useId();
  const owner = dirtyOwner ?? `config-editor-${generatedOwner}`;

  useConfigPageLoad({
    hasConfig: hasData,
    loading,
    loadConfig: load,
    canLoad,
  });

  const markDirty = useCallback(() => {
    setDirtyFor(owner, true);
  }, [owner, setDirtyFor]);

  const markClean = useCallback(() => {
    setDirtyFor(owner, false);
  }, [owner, setDirtyFor]);

  useEffect(() => {
    return () => clearDirtyFor(owner);
  }, [clearDirtyFor, owner]);

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
