import type {
  AccountFieldState,
  ProviderCreateFieldSchema,
  ProviderFieldOption,
  ProviderFieldSchema,
} from "../types/accountConfig";

type ProviderFieldTranslator = (key: string, options?: { defaultValue?: string }) => string;

type ProviderFieldLike =
  | ProviderCreateFieldSchema
  | ProviderFieldSchema
  | AccountFieldState;

type LocalizedOption = ProviderFieldOption & { label: string };
type LocalizedField<TField extends ProviderFieldLike> = TField & {
  label: string;
  description: string;
  options?: LocalizedOption[];
};

function translateKey(
  t: ProviderFieldTranslator,
  key: string | undefined,
  fallback: string,
): string {
  const trimmed = key?.trim() ?? "";
  if (!trimmed) return fallback;
  const translated = t(trimmed, { defaultValue: trimmed });
  return translated === trimmed ? fallback : translated;
}

export function localizeProviderFieldLabel(
  t: ProviderFieldTranslator,
  field: Pick<ProviderFieldLike, "key" | "label_key">,
): string {
  return translateKey(t, field.label_key, field.key);
}

export function localizeProviderFieldDescription(
  t: ProviderFieldTranslator,
  field: Pick<ProviderFieldLike, "description_key">,
): string {
  return translateKey(t, field.description_key, "");
}

export function localizeProviderFieldOption(
  t: ProviderFieldTranslator,
  option: ProviderFieldOption,
): LocalizedOption {
  return {
    ...option,
    label: translateKey(t, option.label_key, option.value),
  };
}

export function localizeProviderField<TField extends ProviderFieldLike>(
  t: ProviderFieldTranslator,
  field: TField,
): LocalizedField<TField> {
  return {
    ...field,
    label: localizeProviderFieldLabel(t, field),
    description: localizeProviderFieldDescription(t, field),
    options: "options" in field ? (field.options ?? []).map((option) => localizeProviderFieldOption(t, option)) : undefined,
  };
}
