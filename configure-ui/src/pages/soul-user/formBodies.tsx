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
          color: "var(--muted)",
          fontSize: "var(--font-size-caption)",
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
              variant={on ? "filled" : "outlined"}
              color={on ? "primary" : "default"}
              sx={{
                borderColor: "var(--border)",
                ...(!on ? { bgcolor: "transparent" } : {}),
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
    <Stack spacing={2}>
      <FormSectionSubCollapsible
        title={t("soulUser.soulGroupBasics")}
        defaultOpen
      >
        <FormFieldStack>
          <TextField
            label={t("soulUser.soulFieldName")}
            value={form.name}
            onChange={(e) => setForm((p) => ({ ...p, name: e.target.value }))}
            size="small"
            fullWidth
            inputProps={{ maxLength: 128 }}
          />
          <FormControl>
            <FormLabel
              sx={{
                fontSize: "var(--font-size-caption)",
                color: "var(--muted)",
                mb: 0.5,
              }}
            >
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
              />
              <FormControlLabel
                value="colloquial"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.colloquial")}
              />
              <FormControlLabel
                value="formal"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.formal")}
              />
              <FormControlLabel
                value="flex"
                control={<Radio size="small" />}
                label={t("soulUser.soulTone.flex")}
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
          size="small"
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
    <Stack spacing={2}>
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
            size="small"
            fullWidth
            inputProps={{ maxLength: 128 }}
          />
          <FormControl>
            <FormLabel
              sx={{
                fontSize: "var(--font-size-caption)",
                color: "var(--muted)",
                mb: 0.5,
              }}
            >
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
              />
              <FormControlLabel
                value="en"
                control={<Radio size="small" />}
                label={t("soulUser.userLang.en")}
              />
              <FormControlLabel
                value="any"
                control={<Radio size="small" />}
                label={t("soulUser.userLang.any")}
              />
            </RadioGroup>
          </FormControl>
          <FormControl>
            <FormLabel
              sx={{
                fontSize: "var(--font-size-caption)",
                color: "var(--muted)",
                mb: 0.5,
              }}
            >
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
              />
              <FormControlLabel
                value="medium"
                control={<Radio size="small" />}
                label={t("soulUser.userReply.medium")}
              />
              <FormControlLabel
                value="long"
                control={<Radio size="small" />}
                label={t("soulUser.userReply.long")}
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
            size="small"
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
            size="small"
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
          size="small"
          fullWidth
          helperText={t("soulUser.userFieldExtraHelp")}
        />
      </FormSectionSubCollapsible>
    </Stack>
  );
}
