import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useRef,
  useState,
  type SyntheticEvent,
} from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import Dialog from "@mui/material/Dialog";
import type { DialogProps } from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import Stack from "@mui/material/Stack";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import AddLink from "@mui/icons-material/AddLink";
import DeleteOutlined from "@mui/icons-material/DeleteOutlined";
import EditOutlined from "@mui/icons-material/EditOutlined";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  FormFieldStack,
  InlineAlert,
  PanelStateBlock,
  PageLoadErrorState,
  PanelStateLoading,
  SectionLoadingSkeleton,
  splitPageErrorState,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SkillMonogramBadge } from "../components/SkillMonogramBadge";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_NAV } from "../config/osIcons";
import { useDeviceApi, type SkillItem } from "../hooks/useDeviceApi";
import { useAppPreferences } from "../hooks/useAppPreferences";
import { useToast } from "../hooks/useToast";
import {
  DIALOG_FOOTER_GUTTER_WIDE_SX,
  MAIN_CONTENT_INNER_SX,
  PAGE_STACK_OUTER_SX,
  TEXT_COLOR,
  TEXT_SUBSECTION_TITLE_SX,
} from "../theme/panelStyles";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { createAsyncState } from "../types/asyncState";
import {
  apiResultIndicatesUnsupportedEndpoint,
  endpointSupportedByInventory,
  parseRootInventory,
} from "../api/rootInventory";
import { SETTINGS_LIST_ROW_PLATE_SX } from "../theme/listItemStyles";
import { createTintedChipSx } from "../theme/chipStyles";
import { CONTENT_MAX_WIDTH } from "../config/layout";
import { translateApiError } from "../i18n/apiErrors";
import { createLatestRequestGuard } from "../util/latestRequest";
import "./skillsMdEditor.css";

const MAX_CONTENT = 32 * 1024;
const SKILL_NAME_PREFIXES = ["runtime_skill__", "runtime_skill_", "skill__", "skill_"];
const SKILL_UPPERCASE_TOKENS = new Set([
  "ai",
  "api",
  "cli",
  "cpu",
  "dns",
  "gpu",
  "http",
  "https",
  "json",
  "llm",
  "mdx",
  "os",
  "pdf",
  "ssh",
  "sql",
  "sse",
  "tcp",
  "tts",
  "udp",
  "ui",
  "url",
  "yaml",
  "xml",
]);

function stripSkillNamePrefix(name: string): string {
  return SKILL_NAME_PREFIXES.find((prefix) => name.startsWith(prefix))
    ? name.slice(
        SKILL_NAME_PREFIXES.find((prefix) => name.startsWith(prefix))!.length,
      )
    : name;
}

function formatSkillToken(token: string): string {
  if (!token) return token;
  const lower = token.toLowerCase();
  if (SKILL_UPPERCASE_TOKENS.has(lower)) return lower.toUpperCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

function humanizeSkillName(name: string): string {
  const trimmed = stripSkillNamePrefix(name);
  const tokens = trimmed.split(/[_-]+/).filter(Boolean);
  if (tokens.length === 0) return name;
  return tokens.map(formatSkillToken).join(" ");
}

function skillKind(name: string): "runtime" | "custom" {
  return name.startsWith("runtime_skill") ? "runtime" : "custom";
}

const SkillRichEditor = lazy(async () => {
  const mod = await import("./skillRichEditor");
  return { default: mod.SkillRichEditor };
});

/** 标题行下方统计：弱对比，避免抢 SettingsSection 主标题 */
const SKILLS_SUMMARY_CHIP_SX = {
  height: 22,
  fontSize: "var(--font-size-caption)",
  fontWeight: 500,
  color: "var(--text-tertiary)",
  borderColor: "color-mix(in srgb, var(--border) 88%, transparent)",
  bgcolor: "transparent",
  backgroundImage: "none",
  boxShadow: "none",
  "& .MuiChip-label": { px: 1, py: 0 },
} as const;

const SKILLS_SUMMARY_ENABLED_CHIP_SX = {
  ...SKILLS_SUMMARY_CHIP_SX,
  ...createTintedChipSx("var(--primary)", {
    height: 22,
    bgStrength: 4,
    borderStrength: 16,
    fontSize: "var(--font-size-caption)",
    fontWeight: 600,
  }),
  boxShadow: "none",
} as const;

export function SkillsPage() {
  const { t } = useTranslation();
  const { themeMode } = useAppPreferences();
  const { showToast } = useToast();
  const { api, ready, canAccessProtectedApis } = useDeviceApi();
  const [listState, setListState] = useState(
    createAsyncState<{ skills: SkillItem[]; order: string[] }>({
      skills: [],
      order: [],
    }),
  );
  const [unsupportedEndpoint, setUnsupportedEndpoint] = useState(false);
  const [editName, setEditName] = useState<string | null>(null);
  const [editContent, setEditContent] = useState("");
  const [editContentInitial, setEditContentInitial] = useState("");
  const [editSaving, setEditSaving] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importUrl, setImportUrl] = useState("");
  const [importName, setImportName] = useState("");
  const [importSaving, setImportSaving] = useState(false);
  const [importError, setImportError] = useState("");
  const [importDiscardOpen, setImportDiscardOpen] = useState(false);
  const [deleteTargetName, setDeleteTargetName] = useState<string | null>(null);
  const [deleteSaving, setDeleteSaving] = useState(false);
  const [discardEditOpen, setDiscardEditOpen] = useState(false);
  const [editBodyLoading, setEditBodyLoading] = useState(false);
  const editLoadGuardRef = useRef(createLatestRequestGuard());

  const loadList = useCallback(async (openOperatorWindow = false) => {
    if (!ready) return;
    setListState((prev) => ({ ...prev, loading: true, error: "" }));
    setUnsupportedEndpoint(false);
    const res = await api.skills.list({
      operatorWindowPolicy: openOperatorWindow ? "auto" : "manual",
    });
    if (res.ok && res.data) {
      setUnsupportedEndpoint(false);
      setListState({
        loading: false,
        error: "",
        data: { skills: res.data.skills, order: res.data.order ?? [] },
      });
    } else {
      let nextError = res.error ?? "";
      let nextUnsupported = false;
      if (
        apiResultIndicatesUnsupportedEndpoint(res) ||
        res.errorKey === "common.not_found" ||
        res.status === 404
      ) {
        const probe = await api.device.probe();
        const inventory = probe.ok ? parseRootInventory(probe.data) : null;
        if (
          apiResultIndicatesUnsupportedEndpoint(res) ||
          !endpointSupportedByInventory(inventory, "GET /api/skills")
        ) {
          nextUnsupported = true;
          nextError = "";
        }
      }
      setUnsupportedEndpoint(nextUnsupported);
      setListState((prev) => ({
        ...prev,
        loading: false,
        error: nextUnsupported ? "" : nextError,
      }));
    }
  }, [api.device, api.skills, ready]);

  useEffect(() => {
    if (!ready) {
      queueMicrotask(() => {
        setUnsupportedEndpoint(false);
        setListState(
          createAsyncState<{ skills: SkillItem[]; order: string[] }>({
            skills: [],
            order: [],
          }),
        );
      });
      return;
    }
    const id = window.setTimeout(() => {
      void loadList();
    }, 0);
    return () => window.clearTimeout(id);
  }, [ready, loadList]);

  useEffect(() => {
    const guard = editLoadGuardRef.current;
    return () => guard.invalidate();
  }, []);

  const handleToggleEnabled = async (name: string, enabled: boolean) => {
    const res = await api.skills.post({ name, enabled });
    if (res.ok)
      setListState((prev) => ({
        ...prev,
        data: {
          ...prev.data,
          skills: prev.data.skills.map((s) =>
            s.name === name ? { ...s, enabled } : s,
          ),
        },
      }));
  };

  const openEdit = async (name: string) => {
    const requestId = editLoadGuardRef.current.next();
    setEditName(name);
    setEditContent("");
    setEditContentInitial("");
    setEditBodyLoading(true);
    const res = await api.skills.getContent(name);
    if (!editLoadGuardRef.current.isCurrent(requestId)) return;
    setEditBodyLoading(false);
    if (res.ok) {
      const content = res.data ?? "";
      setEditContent(content);
      setEditContentInitial(content);
    }
  };

  /** MDX 挂载后会做一次规范化 onChange；同步 initial，避免未编辑点取消仍提示放弃。 */
  const handleEditContentChange = useCallback(
    (markdown: string, initialMarkdownNormalize?: boolean) => {
      setEditContent(markdown);
      if (initialMarkdownNormalize) {
        setEditContentInitial(markdown);
      }
    },
    [],
  );

  const handleSaveEdit = async () => {
    if (!editName || editContent.length > MAX_CONTENT) return;
    setEditSaving(true);
    const res = await api.skills.post({ name: editName, content: editContent });
    setEditSaving(false);
    if (res.ok) {
      setEditContentInitial(editContent);
      setEditName(null);
      showToast(t("common.saveOk"), { variant: "success" });
    } else {
      showToast(translateApiError(t, res.error, "common.error"), { variant: "error" });
    }
  };

  const requestDelete = (name: string) => setDeleteTargetName(name);
  const confirmDelete = async () => {
    if (!deleteTargetName) return;
    setDeleteSaving(true);
    const res = await api.skills.delete(deleteTargetName);
    setDeleteSaving(false);
    setDeleteTargetName(null);
    if (res.ok) {
      showToast(t("skills.deleteOk"), { variant: "success" });
      loadList(true);
    } else {
      showToast(translateApiError(t, res.error, "common.error"), { variant: "error" });
    }
  };

  const closeEditDialog = () => {
    if (editBodyLoading) return;
    if (editContent !== editContentInitial) {
      setDiscardEditOpen(true);
      return;
    }
    setEditName(null);
  };
  const confirmDiscardEdit = () => {
    setDiscardEditOpen(false);
    setEditName(null);
  };

  const closeImportDialogFully = useCallback(() => {
    setImportOpen(false);
    setImportUrl("");
    setImportName("");
    setImportError("");
    setImportDiscardOpen(false);
  }, []);

  const requestCloseImport = useCallback(() => {
    if (importSaving) return;
    if (importUrl.trim() || importName.trim()) {
      setImportDiscardOpen(true);
      return;
    }
    closeImportDialogFully();
  }, [importSaving, importUrl, importName, closeImportDialogFully]);

  const handleImportDialogClose: DialogProps["onClose"] = useCallback(
    (_e: SyntheticEvent, reason: string) => {
      if (importSaving) return;
      if (reason === "backdropClick" || reason === "escapeKeyDown") {
        requestCloseImport();
      }
    },
    [importSaving, requestCloseImport],
  );

  const handleImport = async () => {
    const url = importUrl.trim();
    const name = importName.trim();
    if (!url || !name) {
      setImportError(t("config.validation.urlAndNameRequired"));
      return;
    }
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      setImportError(t("config.validation.urlMustBeHttp"));
      return;
    }
    if (name.includes("..") || name.includes("/") || name.includes("\\")) {
      setImportError(t("config.validation.skillNameInvalid"));
      return;
    }
    if (!ready || !canAccessProtectedApis) {
      setImportError(t("device.pairingCodeRequired"));
      return;
    }
    setImportSaving(true);
    setImportError("");
    const res = await api.skills.import(url, name);
    setImportSaving(false);
    if (res.ok) {
      closeImportDialogFully();
      showToast(t("skills.importOk"), { variant: "success" });
      loadList(true);
    } else {
      const message = translateApiError(t, res.error, "common.error");
      setImportError(message);
      showToast(message, { variant: "error" });
    }
  };

  const skills = listState.data.skills;
  const order = listState.data.order;
  const displayOrder = order.length ? order : skills.map((s) => s.name);
  const orderedSkills = displayOrder
    .map((name) => skills.find((s) => s.name === name))
    .filter((s): s is SkillItem => !!s);
  const missingFromOrder = skills.filter((s) => !displayOrder.includes(s.name));
  const listToShow = [...orderedSkills, ...missingFromOrder];
  const enabledCount = listToShow.filter((skill) => skill.enabled).length;
  const listErrorState = splitPageErrorState({
    hasData: listToShow.length > 0,
    loading: listState.loading,
    error: listState.error,
    suppress: unsupportedEndpoint,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={listErrorState.inlineError} onRetry={() => loadList(true)} />
      <SettingsSection
        pinHeader
        surfaceTone={listState.loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/skills"]} />}
        label={t("skills.sectionList")}
        description={t("skills.sectionListDesc")}
        belowTitleRow={
          !listState.loading && listToShow.length > 0 ? (
            <Stack
              direction="row"
              spacing={0.75}
              useFlexGap
              flexWrap="wrap"
              sx={{ opacity: 0.92 }}
            >
              <Chip
                size="small"
                variant="outlined"
                label={t("skills.summaryTotal", { count: listToShow.length })}
                sx={SKILLS_SUMMARY_CHIP_SX}
              />
              <Chip
                size="small"
                variant="outlined"
                label={t("skills.summaryEnabled", { count: enabledCount })}
                sx={SKILLS_SUMMARY_ENABLED_CHIP_SX}
              />
            </Stack>
          ) : null
        }
        accessory={
          <Button
            size="small"
            variant="outlined"
            startIcon={<AddLink />}
            onClick={() => {
              setImportOpen(true);
              setImportError("");
            }}
            disabled={unsupportedEndpoint}
            sx={{
              borderRadius: "var(--radius-control)",
              ...TEXT_SUBSECTION_TITLE_SX,
            }}
          >
            {t("skills.importFromUrl")}
          </Button>
        }
      >
        {listState.loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : unsupportedEndpoint ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/skills"]} variant="inline" />}
            title={t("skills.unsupportedTitle")}
            description={t("skills.unsupportedDesc")}
          />
        ) : listErrorState.blockingError ? (
          <PageLoadErrorState
            message={listErrorState.blockingError}
            onRetry={() => loadList(true)}
          />
        ) : listToShow.length === 0 ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/skills"]} />}
            title={t("skills.emptyList")}
          />
        ) : (
          <List
            disablePadding
            sx={{
              display: "grid",
              gap: LAYOUT_TOKENS.spacingInlineTight,
              width: "100%",
              pr: 0.5,
              alignContent: "start",
              gridTemplateColumns: {
                xs: "minmax(0, 1fr)",
                xl: "repeat(2, minmax(0, 1fr))",
              },
            }}
          >
            {listToShow.map((skill) => (
              <ListItem key={skill.name} disablePadding sx={{ display: "block" }}>
                <Box
                  sx={{
                    ...SETTINGS_LIST_ROW_PLATE_SX,
                    px: { xs: 1.75, sm: 2 },
                    py: 1.25,
                    display: "grid",
                    gridTemplateColumns: {
                      xs: "minmax(0, 1fr)",
                      sm: "minmax(0, 1fr) auto",
                    },
                    columnGap: 1.5,
                    rowGap: 0.75,
                    alignItems: "center",
                  }}
                >
                  <Box
                    sx={{
                      display: "flex",
                      alignItems: "center",
                      gap: 1.5,
                      minWidth: 0,
                    }}
                  >
                    <SkillMonogramBadge name={skill.name} />
                    <Box
                      sx={{
                        minWidth: 0,
                        display: "grid",
                        gap: 0.5,
                        alignContent: "center",
                      }}
                    >
                      <Box
                        sx={{
                          minWidth: 0,
                        }}
                      >
                        <Typography
                          title={humanizeSkillName(skill.name)}
                          sx={{
                            ...TEXT_SUBSECTION_TITLE_SX,
                            minWidth: 0,
                            color: "var(--text-primary)",
                            fontSize: "var(--font-size-body-sm)",
                            fontWeight: 600,
                            lineHeight: 1.35,
                            whiteSpace: "nowrap",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                          }}
                        >
                          {humanizeSkillName(skill.name)}
                        </Typography>
                      </Box>
                      <Box
                        sx={{
                          display: "flex",
                          alignItems: "center",
                          gap: 0.625,
                          minWidth: 0,
                        }}
                      >
                        <Box
                          component="span"
                          sx={{
                            flexShrink: 0,
                            px: 0.6,
                            py: 0.22,
                            borderRadius: 999,
                            fontSize: "0.7rem",
                            fontWeight: 600,
                            letterSpacing: "0.02em",
                            lineHeight: 1,
                            color:
                              skillKind(skill.name) === "runtime"
                                ? "color-mix(in srgb, var(--primary) 72%, var(--text-secondary))"
                                : "color-mix(in srgb, var(--accent) 70%, var(--text-secondary))",
                            backgroundColor:
                              skillKind(skill.name) === "runtime"
                                ? "color-mix(in srgb, var(--primary) 6%, transparent)"
                                : "color-mix(in srgb, var(--accent) 7%, transparent)",
                            border:
                              skillKind(skill.name) === "runtime"
                                ? "1px solid color-mix(in srgb, var(--primary) 12%, transparent)"
                                : "1px solid color-mix(in srgb, var(--accent) 14%, transparent)",
                          }}
                        >
                          {t(
                            skillKind(skill.name) === "runtime"
                              ? "skills.kindRuntime"
                              : "skills.kindCustom",
                          )}
                        </Box>
                        <Typography
                          title={skill.name}
                          sx={{
                            minWidth: 0,
                            flex: "1 1 auto",
                            fontFamily: "var(--font-mono)",
                            fontSize: "0.76rem",
                            lineHeight: 1.35,
                            color: "var(--text-tertiary)",
                            whiteSpace: "nowrap",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                          }}
                        >
                          {skill.name}
                        </Typography>
                      </Box>
                    </Box>
                  </Box>

                  <Box
                    sx={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "flex-end",
                      gap: 0.625,
                      flexShrink: 0,
                      minWidth: 108,
                      gridColumn: { xs: "1 / -1", sm: "auto" },
                      justifySelf: "end",
                    }}
                  >
                    <Box
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        gap: 0.125,
                        px: 0.3,
                        py: 0.25,
                        borderRadius: "calc(var(--radius-control) + 2px)",
                        border:
                          "1px solid color-mix(in srgb, #fff 42%, var(--border))",
                        backgroundColor:
                          "color-mix(in srgb, var(--surface) 84%, var(--card))",
                        backgroundImage:
                          "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, transparent 100%)",
                        boxShadow: "var(--os3d-control-soft-lift-stack)",
                      }}
                    >
                      <IconButton
                        size="small"
                        onClick={() => openEdit(skill.name)}
                        sx={{
                          color: TEXT_COLOR.secondary,
                          width: 28,
                          height: 28,
                          "&:hover": {
                            color: TEXT_COLOR.primary,
                            backgroundColor:
                              "color-mix(in srgb, var(--foreground) 4%, transparent)",
                          },
                        }}
                        aria-label={t("common.edit")}
                      >
                        <EditOutlined fontSize="small" />
                      </IconButton>
                      <IconButton
                        size="small"
                        onClick={() => requestDelete(skill.name)}
                        sx={{
                          color:
                            "color-mix(in srgb, var(--semantic-danger) 74%, var(--foreground))",
                          width: 28,
                          height: 28,
                          "&:hover": {
                            color: "var(--semantic-danger)",
                            backgroundColor:
                              "color-mix(in srgb, var(--semantic-danger) 8%, transparent)",
                          },
                        }}
                        aria-label={t("common.remove")}
                      >
                        <DeleteOutlined fontSize="small" />
                      </IconButton>
                    </Box>
                    <Switch
                      checked={skill.enabled}
                      onChange={(_, checked) =>
                        handleToggleEnabled(skill.name, checked)
                      }
                      size="small"
                      inputProps={{
                        "aria-label": t(
                          skill.enabled
                            ? "skills.disableSkill"
                            : "skills.enableSkill",
                          { name: skill.name },
                        ),
                      }}
                      sx={{
                        ml: 0.125,
                        "& .MuiSwitch-switchBase": {
                          borderRadius: "var(--radius-control)",
                        },
                      }}
                    />
                  </Box>
                </Box>
              </ListItem>
            ))}
          </List>
        )}
      </SettingsSection>

      <ConfirmDialog
        open={!!deleteTargetName}
        onClose={() => !deleteSaving && setDeleteTargetName(null)}
        title={t("skills.deleteConfirmTitle")}
        description={
          deleteTargetName
            ? t("skills.deleteConfirmDesc", { name: deleteTargetName })
            : ""
        }
        dialogIcon="delete"
        confirmColor="error"
        confirmLabel={t("common.remove")}
        confirmDisabled={deleteSaving}
        onConfirm={confirmDelete}
      />
      <ConfirmDialog
        open={discardEditOpen}
        onClose={() => setDiscardEditOpen(false)}
        title={t("skills.discardEditTitle")}
        description={t("skills.discardEditDesc")}
        dialogIcon="unsavedChanges"
        confirmColor="error"
        confirmLabel={t("common.confirm")}
        onConfirm={confirmDiscardEdit}
      />
      <ConfirmDialog
        open={importDiscardOpen}
        onClose={() => setImportDiscardOpen(false)}
        title={t("skills.discardImportTitle")}
        description={t("skills.discardImportDesc")}
        dialogIcon="unsavedChanges"
        confirmColor="error"
        confirmLabel={t("common.confirm")}
        onConfirm={closeImportDialogFully}
      />
      <Dialog
        open={!!editName}
        onClose={() => !editSaving && !editBodyLoading && closeEditDialog()}
        maxWidth={false}
        fullWidth
        scroll="paper"
        slotProps={{
          backdrop: {
            sx: {
              backgroundColor: "var(--backdrop-overlay)",
              backdropFilter: "blur(var(--glass-blur))",
              WebkitBackdropFilter: "blur(var(--glass-blur))",
            },
          },
          paper: {
            sx: {
              width: "100%",
              maxWidth: `min(${CONTENT_MAX_WIDTH}px, calc(100vw - 24px))`,
              borderRadius: "var(--radius-card)",
              border: "1px solid var(--form-outline-rest)",
              backgroundColor: "var(--card)",
              boxShadow: "var(--os3d-content-plate-stack)",
              display: "flex",
              flexDirection: "column",
              maxHeight: "calc(100vh - 16px)",
            },
          },
        }}
        sx={{
          "& .MuiDialog-container": {
            alignItems: "center",
          },
        }}
      >
        <DialogTitle
          component="div"
          sx={{
            ...MAIN_CONTENT_INNER_SX,
            flexShrink: 0,
            pt: 4,
            pb: 1.5,
            backgroundColor: "var(--card)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            textAlign: "center",
          }}
        >
          {editName ? (
            <Typography
              component="h1"
              sx={{
                fontFamily: "var(--font-mono)",
                fontSize: {
                  xs: "var(--font-size-h2)",
                  sm: "var(--font-size-h1)",
                },
                fontWeight: 800,
                color: "var(--text-primary)",
                lineHeight: "var(--line-height-tight)",
                letterSpacing: "var(--letter-spacing-tight)",
                wordBreak: "break-all",
                textAlign: "center",
              }}
            >
              {editName}
            </Typography>
          ) : (
            <Typography
              component="h1"
              sx={{
                fontSize: {
                  xs: "var(--font-size-h2)",
                  sm: "var(--font-size-h1)",
                },
                fontWeight: 800,
                color: "var(--text-primary)",
                textAlign: "center",
                letterSpacing: "var(--letter-spacing-tight)",
              }}
            >
              {t("skills.editSkillDialogTitle")}
            </Typography>
          )}
        </DialogTitle>
        <DialogContent
          sx={{
            flex: "1 1 auto",
            minHeight: 0,
            overflow: "hidden",
            display: "flex",
            flexDirection: "column",
            p: 0,
          }}
        >
          <Box
            sx={{
              flex: 1,
              minHeight: 0,
              display: "flex",
              flexDirection: "column",
              bgcolor: "var(--card)",
              p: 0,
              overflow: "hidden",
            }}
          >
            {/**
             * 唯一滚动层：固定 max-height + overflow:auto，滚轮作用在本层；
             * Lexical 编辑区不再作为滚动容器，避免吞掉滚轮。
             */}
            <Box
              className="skill-edit-scroll skill-edit-scroll--immersive"
              sx={{
                flex: 1,
                minHeight: 0,
                px: 0,
                maxHeight: {
                  xs: "min(82vh, calc(100vh - 140px))",
                  sm: "min(80vh, calc(100vh - 150px))",
                },
              }}
            >
              {editBodyLoading ? (
                <Box
                  sx={{
                    minHeight: { xs: 200, sm: 240 },
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    py: 2,
                  }}
                >
                  <CircularProgress
                    size={32}
                    sx={{ color: "var(--primary)" }}
                  />
                </Box>
              ) : (
                <Suspense
                  fallback={
                    <Box
                      sx={{
                        minHeight: { xs: 200, sm: 240 },
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        py: 2,
                      }}
                    >
                      <CircularProgress
                        size={32}
                        sx={{ color: "var(--primary)" }}
                      />
                    </Box>
                  }
                >
                  <SkillRichEditor
                    key={editName ?? "skill-edit"}
                    markdown={editContent}
                    onChange={handleEditContentChange}
                    themeMode={themeMode}
                  />
                </Suspense>
              )}
            </Box>
          </Box>
        </DialogContent>
        <DialogActions
          sx={{
            ...DIALOG_FOOTER_GUTTER_WIDE_SX,
            flexShrink: 0,
            py: 2,
            gap: LAYOUT_TOKENS.spacingTitleToContent,
            flexWrap: "wrap",
            justifyContent: "space-between",
            alignItems: "center",
            backgroundColor:
              "color-mix(in srgb, var(--surface) 85%, transparent)",
            backdropFilter: "blur(var(--overlay-backdrop-blur))",
            WebkitBackdropFilter: "blur(var(--overlay-backdrop-blur))",
            borderTop: "1px solid var(--border-subtle)",
          }}
        >
          <Typography
            variant="caption"
            component="span"
            sx={{
              color:
                editContent.length > MAX_CONTENT
                  ? "var(--semantic-danger)"
                  : "var(--text-tertiary)",
              fontWeight: editContent.length > MAX_CONTENT ? 600 : 500,
              fontVariantNumeric: "tabular-nums",
              letterSpacing: "var(--letter-spacing-label)",
            }}
          >
            {t("skills.editCharCount", {
              current: editContent.length,
              max: MAX_CONTENT,
            })}
          </Typography>
          <Box
            sx={{
              display: "flex",
              gap: LAYOUT_TOKENS.spacingInlineTight,
              flexWrap: "wrap",
              ml: { xs: 0, sm: "auto" },
            }}
          >
            <Button
              variant="text"
              onClick={closeEditDialog}
              disabled={editSaving || editBodyLoading}
              sx={{
                borderRadius: "var(--radius-control)",
                textTransform: "none",
                fontWeight: 600,
                color: "var(--text-tertiary)",
                "&:hover": {
                  bgcolor:
                    "color-mix(in srgb, var(--foreground) 6%, transparent)",
                },
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="contained"
              onClick={handleSaveEdit}
              disableElevation
              disabled={
                editSaving ||
                editBodyLoading ||
                editContent.length > MAX_CONTENT
              }
              sx={{
                borderRadius: "var(--radius-control)",
                textTransform: "none",
                fontWeight: 600,
                boxShadow: "none",
                "&:hover": { boxShadow: "none" },
              }}
            >
              {editSaving ? t("common.saving") : t("common.save")}
            </Button>
          </Box>
        </DialogActions>
      </Dialog>

      <Dialog
        open={importOpen}
        onClose={handleImportDialogClose}
        maxWidth="sm"
        fullWidth
        slotProps={{
          paper: {
            sx: {
              borderRadius: "var(--radius-card)",
              border: "1px solid var(--form-outline-rest)",
              backgroundColor: "var(--card)",
              boxShadow: "var(--os3d-content-plate-stack)",
            },
          },
        }}
      >
        <DialogTitle sx={{ ...TEXT_SUBSECTION_TITLE_SX, fontWeight: 700 }}>
          {t("skills.importFromUrl")}
        </DialogTitle>
        <DialogContent>
          <FormFieldStack>
            <TextField
              label={t("skills.importUrlLabel")}
              value={importUrl}
              onChange={(e) => setImportUrl(e.target.value)}
              placeholder={t("skills.importUrlPlaceholder")}
              fullWidth
              slotProps={{
                htmlInput: { style: { fontFamily: "var(--font-mono)" } },
              }}
            />
            <TextField
              label={t("skills.importNameLabel")}
              value={importName}
              onChange={(e) => setImportName(e.target.value)}
              fullWidth
              slotProps={{
                htmlInput: { style: { fontFamily: "var(--font-mono)" } },
              }}
            />
          </FormFieldStack>
          {importError && (
            <Typography
              variant="body2"
              sx={{ color: "var(--semantic-danger)", mt: 2, fontWeight: 500 }}
            >
              {importError}
            </Typography>
          )}
        </DialogContent>
        <DialogActions sx={{ ...MAIN_CONTENT_INNER_SX, pb: 2 }}>
          <Button
            onClick={requestCloseImport}
            disabled={importSaving}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="contained"
            onClick={handleImport}
            disabled={importSaving}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {importSaving ? t("common.saving") : t("common.import")}
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}
