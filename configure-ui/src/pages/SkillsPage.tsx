import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
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
import ExtensionOutlined from "@mui/icons-material/ExtensionOutlined";
import WarningAmberRounded from "@mui/icons-material/WarningAmberRounded";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  FormFieldStack,
  InlineAlert,
  SectionLoadingSkeleton,
} from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
import { useDeviceApi, type SkillItem } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import { createAsyncState } from "../types/asyncState";
import {
  SETTINGS_SECTION_LIST_EMPTY_SX,
  SETTINGS_SECTION_LIST_ROW_SX,
} from "../theme/listItemStyles";

const MAX_CONTENT = 32 * 1024;

export function SkillsPage() {
  const { t } = useTranslation();
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
  const editContentInputRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (!editName) return;
    const id = setTimeout(() => editContentInputRef.current?.focus(), 120);
    return () => clearTimeout(id);
  }, [editName]);

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
      setListState((prev) => ({
        ...prev,
        loading: false,
        error: res.error ?? "",
      }));
    }
  }, [api.skills, ready]);

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
    const res = await api.skills.getContent(name);
    if (res.ok) {
      const content = res.data ?? "";
      setEditContent(content);
      setEditContentInitial(content);
    }
  };

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
    if (
      editContent !== editContentInitial &&
      editContentInitial !== undefined
    ) {
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
    (_e, reason) => {
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
    <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <InlineAlert message={listState.error || null} onRetry={loadList} />
      <SettingsSection
        icon={<ExtensionOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
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
              fontSize: "var(--font-size-body-sm)",
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
                    sx: {
                      color: "var(--muted)",
                      fontSize: "var(--font-size-caption)",
                    },
                  },
                }}
              />
            </ListItem>
          </List>
        ) : (
          <List
            dense
            disablePadding
            sx={{ display: "flex", flexDirection: "column", gap: 1 }}
          >
            {listToShow.map((skill) => (
              <ListItem
                key={skill.name}
                sx={{
                  ...SETTINGS_SECTION_LIST_ROW_SX,
                  display: "flex",
                  gap: 1,
                }}
                secondaryAction={
                  <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                    <IconButton
                      size="small"
                      onClick={() => openEdit(skill.name)}
                      sx={{ color: "var(--muted)" }}
                      aria-label={t("common.edit")}
                    >
                      <EditOutlined fontSize="small" />
                    </IconButton>
                    <IconButton
                      size="small"
                      onClick={() => requestDelete(skill.name)}
                      sx={{ color: "var(--muted)" }}
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
                        fontSize: "var(--font-size-body-sm)",
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
        onClose={() => !editSaving && closeEditDialog()}
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
        <DialogTitle
          sx={{ fontSize: "var(--font-size-body-sm)", fontWeight: 700 }}
        >
          {editName ? t("skills.editSkill", { name: editName }) : ""}
        </DialogTitle>
        <DialogContent>
          <TextField
            inputRef={editContentInputRef}
            multiline
            minRows={8}
            maxRows={20}
            fullWidth
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            size="small"
            helperText={t("skills.editCharCount", {
              current: editContent.length,
              max: MAX_CONTENT,
            })}
            sx={{
              mt: 1,
              "& textarea": {
                fontFamily: "var(--font-mono)",
                fontSize: "var(--font-size-body)",
              },
            }}
          />
        </DialogContent>
        <DialogActions sx={{ px: 2.5, pb: 2 }}>
          <Button
            onClick={closeEditDialog}
            disabled={editSaving}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="contained"
            onClick={handleSaveEdit}
            disabled={editSaving}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {editSaving ? t("common.saving") : t("common.save")}
          </Button>
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
        <DialogTitle
          sx={{ fontSize: "var(--font-size-body-sm)", fontWeight: 700 }}
        >
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
              size="small"
              slotProps={{
                htmlInput: { style: { fontFamily: "var(--font-mono)" } },
              }}
            />
            <TextField
              label={t("skills.importNameLabel")}
              value={importName}
              onChange={(e) => setImportName(e.target.value)}
              fullWidth
              size="small"
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
        <DialogActions sx={{ px: 2.5, pb: 2 }}>
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
