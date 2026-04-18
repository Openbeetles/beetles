import type { Dispatch, SetStateAction } from "react";
import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormLabel from "@mui/material/FormLabel";
import Radio from "@mui/material/Radio";
import RadioGroup from "@mui/material/RadioGroup";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import {
  FormFieldStack,
  FormSectionSubCollapsible,
} from "../../components/form";
import { TEXT_BODY_TERTIARY_SX } from "../../theme/panelStyles";
import { LAYOUT_TOKENS } from "../../config/themeTokens";
import {
  SOUL_SKILL_KEYS,
  SOUL_TRAIT_KEYS,
  toggleMultiValue,
  type SoulFormState,
  type SoulTone,
  type UserFormState,
  USER_INTEREST_KEYS,
  type UserLangPref,
  type UserReplyLength,
} from "../../util/soulUserFormat";

const SOUL_OPTION_CHIP_SX = {
  height: 32,
  borderRadius: "calc(var(--radius-chip) + 1px)",
  fontWeight: 600,
  fontSize: "var(--font-size-label)",
  borderColor: "color-mix(in srgb, var(--border) 18%, transparent)",
  backgroundColor: "color-mix(in srgb, var(--card) 78%, var(--surface))",
  boxShadow: "var(--os3d-control-soft-lift-stack)",
  "& .MuiChip-label": {
    px: 1.15,
  },
  "&:hover": {
    backgroundColor: "color-mix(in srgb, var(--primary) 4%, var(--card))",
    borderColor: "color-mix(in srgb, var(--border) 28%, transparent)",
  },
} as const;

const SOUL_OPTION_CHIP_SELECTED_SX = {
  color: "color-mix(in srgb, var(--primary) 82%, var(--foreground))",
  backgroundColor: "color-mix(in srgb, var(--primary) 9%, var(--card))",
  borderColor: "color-mix(in srgb, var(--primary) 20%, var(--border))",
  boxShadow: "var(--os3d-selection-pill-stack)",
  "&:hover": {
    backgroundColor: "color-mix(in srgb, var(--primary) 11%, var(--card))",
    borderColor: "color-mix(in srgb, var(--primary) 24%, var(--border))",
  },
} as const;

const SOUL_RADIO_OPTION_SX = {
  m: 0,
  px: 0.9,
  py: 0.3,
  borderRadius: "calc(var(--radius-control) + 1px)",
  border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
  backgroundColor: "color-mix(in srgb, var(--card) 78%, var(--surface))",
  boxShadow: "var(--os3d-control-soft-lift-stack)",
  transition:
    "background-color var(--transition-duration) ease, border-color var(--transition-duration) ease, box-shadow var(--transition-duration) ease",
  "& .MuiFormControlLabel-label": {
    fontSize: "var(--font-size-body-sm)",
    fontWeight: 600,
    color: "var(--text-secondary)",
  },
  "& .MuiRadio-root": {
    p: 0.45,
    mr: 0.375,
  },
  "&:hover": {
    backgroundColor: "color-mix(in srgb, var(--primary) 4%, var(--card))",
    borderColor: "color-mix(in srgb, var(--border) 28%, transparent)",
  },
  "&:has(.MuiRadio-root.Mui-checked)": {
    backgroundColor: "color-mix(in srgb, var(--primary) 8%, var(--card))",
    borderColor: "color-mix(in srgb, var(--primary) 22%, var(--border))",
    boxShadow: "var(--os3d-selection-pill-stack)",
  },
  "&:has(.MuiRadio-root.Mui-checked) .MuiFormControlLabel-label": {
    color: "color-mix(in srgb, var(--primary) 80%, var(--foreground))",
    fontWeight: 700,
  },
} as const;

export function ChipSelectRow({
  label,
  keys,
  i18nPrefix,
  selected,
  onToggle,
  t,
}: {
  label: string;
  keys: readonly string[];
  i18nPrefix: string;
  selected: readonly string[];
  onToggle: (key: string) => void;
  t: (k: string) => string;
}) {
  return (
    <Box>
      <Typography
        component="p"
        sx={{
          mb: 1,
          ...TEXT_BODY_TERTIARY_SX,
          fontWeight: 600,
        }}
      >
        {label}
      </Typography>
      <Stack direction="row" flexWrap="wrap" useFlexGap gap={1}>
        {keys.map((key) => {
          const on = selected.includes(key);
          return (
            <Chip
              key={key}
              label={t(`${i18nPrefix}.${key}`)}
              onClick={() => onToggle(key)}
              variant="outlined"
              sx={{
                ...SOUL_OPTION_CHIP_SX,
                ...(on ? SOUL_OPTION_CHIP_SELECTED_SX : null),
              }}
            />
          );
        })}
      </Stack>
    </Box>
  );
}

export function SoulFormBody({
  form,
  setForm,
  t,
}: {
  form: SoulFormState;
  setForm: Dispatch<SetStateAction<SoulFormState>>;
  t: (k: string) => string;
}) {
  return (
    <Stack spacing={LAYOUT_TOKENS.spacingSectionStack}>
      <FormSectionSubCollapsible
        title={t("soulUser.soulGroupBasics")}
        defaultOpen
      >
        <FormFieldStack>
          <TextField
            label={t("soulUser.soulFieldName")}
            value={form.name}
            onChange={(e) => setForm((p) => ({ ...p, name: e.target.value }))}
            fullWidth
            inputProps={{ maxLength: 128 }}
          />
          <FormControl>
            <FormLabel sx={{ ...TEXT_BODY_TERTIARY_SX, mb: 0.5 }}>
              {t("soulUser.soulFieldTone")}
            </FormLabel>
            <RadioGroup
              row
              value={form.tone || "none"}
              onChange={(e) => {
                const v = e.target.value;
                setForm((p) => ({
                  ...p,
                  tone: v === "none" ? "" : (v as SoulTone),
                }));
              }}
              sx={{ flexWrap: "wrap", gap: 0.5 }}
            >
              <FormControlLabel
                value="none"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.none")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="colloquial"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.colloquial")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="formal"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.formal")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="flex"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.flex")}
                sx={SOUL_RADIO_OPTION_SX}
              />
            </RadioGroup>
          </FormControl>
        </FormFieldStack>
      </FormSectionSubCollapsible>

      <FormSectionSubCollapsible
        title={t("soulUser.soulGroupStyle")}
        defaultOpen
      >
        <FormFieldStack>
          <ChipSelectRow
            label={t("soulUser.soulFieldTraits")}
            keys={SOUL_TRAIT_KEYS}
            i18nPrefix="soulUser.soulTrait"
            selected={form.traits}
            onToggle={(key) =>
              setForm((p) => ({
                ...p,
                traits: toggleMultiValue(p.traits, key),
              }))
            }
            t={t}
          />
          <ChipSelectRow
            label={t("soulUser.soulFieldSkills")}
            keys={SOUL_SKILL_KEYS}
            i18nPrefix="soulUser.soulSkill"
            selected={form.skills}
            onToggle={(key) =>
              setForm((p) => ({
                ...p,
                skills: toggleMultiValue(p.skills, key),
              }))
            }
            t={t}
          />
        </FormFieldStack>
      </FormSectionSubCollapsible>

      <FormSectionSubCollapsible
        title={t("soulUser.soulGroupExtra")}
        defaultOpen={false}
      >
        <TextField
          label={t("soulUser.soulFieldExtra")}
          value={form.extra}
          onChange={(e) => setForm((p) => ({ ...p, extra: e.target.value }))}
          multiline
          minRows={3}
          maxRows={8}
          fullWidth
          helperText={t("soulUser.soulFieldExtraHelp")}
        />
      </FormSectionSubCollapsible>
    </Stack>
  );
}

export function UserFormBody({
  form,
  setForm,
  t,
}: {
  form: UserFormState;
  setForm: Dispatch<SetStateAction<UserFormState>>;
  t: (k: string) => string;
}) {
  return (
    <Stack spacing={LAYOUT_TOKENS.spacingSectionStack}>
      <FormSectionSubCollapsible
        title={t("soulUser.userGroupBasics")}
        defaultOpen
      >
        <FormFieldStack>
          <TextField
            label={t("soulUser.userFieldNickname")}
            value={form.nickname}
            onChange={(e) =>
              setForm((p) => ({ ...p, nickname: e.target.value }))
            }
            fullWidth
            inputProps={{ maxLength: 128 }}
          />
          <FormControl>
            <FormLabel sx={{ ...TEXT_BODY_TERTIARY_SX, mb: 0.5 }}>
              {t("soulUser.userFieldLang")}
            </FormLabel>
            <RadioGroup
              row
              value={form.langPref}
              onChange={(e) =>
                setForm((p) => ({
                  ...p,
                  langPref: e.target.value as UserLangPref,
                }))
              }
              sx={{ flexWrap: "wrap", gap: 0.5 }}
            >
              <FormControlLabel
                value="zh"
                control={<Radio size="small" />}
                label={t("soulUser.userLang.zh")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="en"
                control={<Radio size="small" />}
                label={t("soulUser.userLang.en")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="any"
                control={<Radio size="small" />}
                label={t("soulUser.userLang.any")}
                sx={SOUL_RADIO_OPTION_SX}
              />
            </RadioGroup>
          </FormControl>
          <FormControl>
            <FormLabel sx={{ ...TEXT_BODY_TERTIARY_SX, mb: 0.5 }}>
              {t("soulUser.userFieldReplyLength")}
            </FormLabel>
            <RadioGroup
              row
              value={form.replyLength}
              onChange={(e) =>
                setForm((p) => ({
                  ...p,
                  replyLength: e.target.value as UserReplyLength,
                }))
              }
              sx={{ flexWrap: "wrap", gap: 0.5 }}
            >
              <FormControlLabel
                value="short"
                control={<Radio size="small" />}
                label={t("soulUser.userReply.short")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="medium"
                control={<Radio size="small" />}
                label={t("soulUser.userReply.medium")}
                sx={SOUL_RADIO_OPTION_SX}
              />
              <FormControlLabel
                value="long"
                control={<Radio size="small" />}
                label={t("soulUser.userReply.long")}
                sx={SOUL_RADIO_OPTION_SX}
              />
            </RadioGroup>
          </FormControl>
        </FormFieldStack>
      </FormSectionSubCollapsible>

      <FormSectionSubCollapsible
        title={t("soulUser.userGroupProfile")}
        defaultOpen
      >
        <FormFieldStack>
          <TextField
            label={t("soulUser.userFieldOccupation")}
            value={form.occupation}
            onChange={(e) =>
              setForm((p) => ({ ...p, occupation: e.target.value }))
            }
            fullWidth
            inputProps={{ maxLength: 256 }}
          />
          <ChipSelectRow
            label={t("soulUser.userFieldInterests")}
            keys={USER_INTEREST_KEYS}
            i18nPrefix="soulUser.userInterest"
            selected={form.interests}
            onToggle={(key) =>
              setForm((p) => ({
                ...p,
                interests: toggleMultiValue(p.interests, key),
              }))
            }
            t={t}
          />
          <TextField
            label={t("soulUser.userFieldTimezone")}
            value={form.timezone}
            onChange={(e) =>
              setForm((p) => ({ ...p, timezone: e.target.value }))
            }
            fullWidth
            placeholder={t("soulUser.userFieldTimezonePlaceholder")}
            inputProps={{ maxLength: 128 }}
          />
        </FormFieldStack>
      </FormSectionSubCollapsible>

      <FormSectionSubCollapsible
        title={t("soulUser.userGroupExtra")}
        defaultOpen={false}
      >
        <TextField
          label={t("soulUser.userFieldExtra")}
          value={form.extra}
          onChange={(e) => setForm((p) => ({ ...p, extra: e.target.value }))}
          multiline
          minRows={3}
          maxRows={8}
          fullWidth
          helperText={t("soulUser.userFieldExtraHelp")}
        />
      </FormSectionSubCollapsible>
    </Stack>
  );
}
