import { useTranslation } from "react-i18next";
import FormControl from "@mui/material/FormControl";
import FormHelperText from "@mui/material/FormHelperText";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import TextField from "@mui/material/TextField";
import type {
  ProviderCreateFieldSchema,
  ProviderFieldOption,
  ProviderFieldSchema,
  ProviderFieldValueKind,
} from "../types/accountConfig";

type BaseFieldSchema = {
  key: string;
  label: string;
  description: string;
  value_kind: ProviderFieldValueKind;
  required: boolean;
  secret: boolean;
  multiple?: boolean;
  options?: ProviderFieldOption[];
};

export interface ProviderFieldInputProps {
  field: ProviderCreateFieldSchema | ProviderFieldSchema | BaseFieldSchema;
  value: string;
  onChange: (value: string) => void;
}

export function ProviderFieldInput({
  field,
  value,
  onChange,
}: ProviderFieldInputProps) {
  const { t } = useTranslation();
  const secret = field.secret || field.value_kind === "secret";
  const multiline =
    "multiple" in field ? Boolean(field.multiple) : false;
  const options = "options" in field ? field.options ?? [] : [];
  if (options.length > 0 || field.value_kind === "boolean") {
    const selectOptions =
      field.value_kind === "boolean"
        ? [
            { value: "", label: t("accounts.providerFieldOption.empty") },
            { value: "true", label: t("accounts.providerFieldOption.true") },
            { value: "false", label: t("accounts.providerFieldOption.false") },
          ]
        : options.map((option) =>
            field.key === "identity_class"
              ? {
                  ...option,
                  label: t(`accounts.identity.${option.value}`, {
                    defaultValue: option.label,
                  }),
                }
              : option,
          );
    return (
      <FormControl fullWidth required={field.required}>
        <InputLabel id={`${field.key}-label`}>{field.label}</InputLabel>
        <Select
          labelId={`${field.key}-label`}
          label={field.label}
          value={value}
          onChange={(event) => onChange(String(event.target.value))}
        >
          {selectOptions.map((option) => (
            <MenuItem key={`${field.key}-${option.value}`} value={option.value}>
              {option.label}
            </MenuItem>
          ))}
        </Select>
        {field.description ? (
          <FormHelperText>{field.description}</FormHelperText>
        ) : null}
      </FormControl>
    );
  }
  return (
    <TextField
      required={field.required}
      fullWidth
      type={secret ? "password" : field.value_kind === "integer" ? "number" : "text"}
      label={field.label}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      helperText={field.description || undefined}
      multiline={multiline}
      minRows={multiline ? 3 : undefined}
      inputProps={{
        autoComplete: secret ? "new-password" : "off",
        spellCheck: false,
        inputMode: field.value_kind === "integer" ? "numeric" : undefined,
      }}
    />
  );
}
