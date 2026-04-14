import { useCallback, useEffect, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import type { DialogProps } from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import AddLink from "@mui/icons-material/AddLink";
import DeleteOutlined from "@mui/icons-material/DeleteOutlined";
import EditOutlined from "@mui/icons-material/EditOutlined";
import WarningAmberRounded from "@mui/icons-material/WarningAmberRounded";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  FormFieldStack,
  InlineAlert,
  SectionLoadingSkeleton,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_NAV } from "../config/osIcons";
import { useDeviceApi, type SkillItem } from "../hooks/useDeviceApi";
import { useAppPreferences } from "../hooks/useAppPreferences";
import { useToast } from "../hooks/useToast";
import {
  DIALOG_FOOTER_GUTTER_WIDE_SX,
  MAIN_CONTENT_INNER_SX,
  PAGE_STACK_OUTER_SX,
  TEXT_BODY_TERTIARY_SX,
  TEXT_COLOR,
  TEXT_SUBSECTION_TITLE_SX,
} from "../theme/panelStyles";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { createAsyncState } from "../types/asyncState";
import {
  endpointSupportedByInventory,
  parseRootInventory,
} from "../api/rootInventory";
import {
  SETTINGS_SECTION_LIST_EMPTY_SX,
  SETTINGS_SECTION_LIST_ROW_SX,
} from "../theme/listItemStyles";
import { CONTENT_MAX_WIDTH } from "../config/layout";
import { SkillRichEditor } from "./skillRichEditor";
import "./skillsMdEditor.css";

const MAX_CONTENT = 32 * 1024;

export function SkillsPage() {
  const { t } = useTranslation();
  const { themeMode } = useAppPreferences();
  const { showToast } = useToast();
  const { api, ready, hasPairing } = useDeviceApi();
  const [listState, setListState] = useState(
    createAsyncState<{ skills: SkillItem[]; order: string[] }>({
      skills: [],
      order: [],
    }),
  );
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

  const loadList = useCallback(async () => {
    if (!ready) return;
    setListState((prev) => ({ ...prev, loading: true, error: "" }));
    const res = await api.skills.list();
    if (res.ok && res.data) {
      setListState({
        loading: false,
        error: "",
        data: { skills: res.data.skills, order: res.data.order ?? [] },
      });
    } else {
      let nextError = res.error ?? "";
      if (nextError === "Not Found" || nextError === "not found") {
        const probe = await api.device.probe();
        const inventory = probe.ok ? parseRootInventory(probe.data) : null;
        if (!endpointSupportedByInventory(inventory, "GET /api/skills")) {
          nextError = t("skills.unsupportedEndpoint");
        }
      }
      setListState((prev) => ({
        ...prev,
        loading: false,
        error: nextError,
      }));
    }
  }, [api.device, api.skills, ready, t]);

  useEffect(() => {
    if (!ready) return;
    const id = window.setTimeout(() => {
      void loadList();
    }, 0);
    return () => window.clearTimeout(id);
  }, [ready, loadList]);

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
    setEditName(name);
    setEditContent("");
    setEditContentInitial("");
    setEditBodyLoading(true);
    const res = await api.skills.getContent(name);
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
      showToast(res.error ?? t("common.error"), { variant: "error" });
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
      loadList();
    } else {
      showToast(res.error ?? t("common.error"), { variant: "error" });
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
    if (!ready || !hasPairing) {
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
      loadList();
    } else {
      setImportError(res.error ?? "");
      showToast(res.error ?? "", { variant: "error" });
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

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={listState.error || null} onRetry={loadList} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/skills"]} />}
        label={t("skills.sectionList")}
        description={t("skills.sectionListDesc")}
        accessory={
          <Button
            size="small"
            variant="outlined"
            startIcon={<AddLink />}
            onClick={() => {
              setImportOpen(true);
              setImportError("");
            }}
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
          <SectionLoadingSkeleton />
        ) : listToShow.length === 0 ? (
          <List dense disablePadding>
            <ListItem sx={SETTINGS_SECTION_LIST_EMPTY_SX}>
              <ListItemText
                primary={t("skills.emptyList")}
                slotProps={{
                  primary: {
                    variant: "body2",
                    sx: TEXT_BODY_TERTIARY_SX,
                  },
                }}
              />
            </ListItem>
          </List>
        ) : (
          <List
            dense
            disablePadding
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: LAYOUT_TOKENS.spacingInlineTight,
            }}
          >
            {listToShow.map((skill) => (
              <ListItem
                key={skill.name}
                sx={{
                  ...SETTINGS_SECTION_LIST_ROW_SX,
                  display: "flex",
                  gap: LAYOUT_TOKENS.spacingInlineTight,
                }}
                secondaryAction={
                  <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                    <IconButton
                      size="small"
                      onClick={() => openEdit(skill.name)}
                      sx={{ color: TEXT_COLOR.tertiary }}
                      aria-label={t("common.edit")}
                    >
                      <EditOutlined fontSize="small" />
                    </IconButton>
                    <IconButton
                      size="small"
                      onClick={() => requestDelete(skill.name)}
                      sx={{ color: TEXT_COLOR.tertiary }}
                      aria-label={t("common.remove")}
                    >
                      <DeleteOutlined fontSize="small" />
                    </IconButton>
                    <Switch
                      checked={skill.enabled}
                      onChange={(_, checked) =>
                        handleToggleEnabled(skill.name, checked)
                      }
                      size="small"
                      sx={{
                        "& .MuiSwitch-switchBase": {
                          borderRadius: "var(--radius-control)",
                        },
                      }}
                    />
                  </Box>
                }
              >
                <ListItemText
                  primary={skill.name}
                  slotProps={{
                    primary: {
                      sx: {
                        ...TEXT_SUBSECTION_TITLE_SX,
                        fontFamily: "var(--font-mono)",
                      },
                    },
                  }}
                />
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
        icon={<DeleteOutlined />}
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
        confirmColor="error"
        confirmLabel={t("common.confirm")}
        onConfirm={confirmDiscardEdit}
      />
      <ConfirmDialog
        open={importDiscardOpen}
        onClose={() => setImportDiscardOpen(false)}
        title={t("skills.discardImportTitle")}
        description={t("skills.discardImportDesc")}
        icon={<WarningAmberRounded />}
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
              border: "none",
              backgroundColor: "var(--surface)",
              boxShadow: "none",
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
            backgroundColor: "var(--surface)",
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
              bgcolor: "var(--surface)",
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
                <SkillRichEditor
                  key={editName ?? "skill-edit"}
                  markdown={editContent}
                  onChange={handleEditContentChange}
                  themeMode={themeMode}
                />
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
              border: "none",
              backgroundColor: "var(--surface)",
              boxShadow: "none",
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
