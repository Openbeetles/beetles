import { useMemo, useState, type ReactNode } from "react";
import Box from "@mui/material/Box";
import Dialog from "@mui/material/Dialog";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import SendRoundedIcon from "@mui/icons-material/SendRounded";
import { useTranslation } from "react-i18next";
import { OS_ICON_NAV } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";

interface ChatDialogProps {
  open: boolean;
  onClose: () => void;
  onMinimize: () => void;
}

interface MockConversation {
  id: string;
  titleKey: string;
  subtitle?: string;
  subtitleKey?: string;
  timeKey?: string;
  time?: string;
  unread?: number;
  online?: boolean;
  messages: MockMessage[];
}

interface MockMessage {
  id: string;
  author: "beetle" | "user";
  text?: string;
  textKey?: string;
  timeKey?: string;
  time?: string;
}

const CHAT_DIALOG_TITLE_ID = "chat-dialog-title";

const INITIAL_CONVERSATIONS: MockConversation[] = [
  {
    id: "main",
    titleKey: "chat.preview.main.title",
    subtitleKey: "chat.preview.main.subtitle",
    time: "12:45",
    unread: 2,
    online: true,
    messages: [
      {
        id: "main-1",
        author: "beetle",
        textKey: "chat.preview.main.message1",
        time: "12:42",
      },
      {
        id: "main-2",
        author: "user",
        textKey: "chat.preview.main.message2",
        time: "12:43",
      },
      {
        id: "main-3",
        author: "beetle",
        textKey: "chat.preview.main.message3",
        time: "12:45",
      },
    ],
  },
  {
    id: "ops",
    titleKey: "chat.preview.ops.title",
    subtitleKey: "chat.preview.ops.subtitle",
    time: "11:58",
    online: true,
    messages: [
      {
        id: "ops-1",
        author: "beetle",
        textKey: "chat.preview.ops.message1",
        time: "11:56",
      },
      {
        id: "ops-2",
        author: "user",
        textKey: "chat.preview.ops.message2",
        time: "11:58",
      },
    ],
  },
  {
    id: "notes",
    titleKey: "chat.preview.notes.title",
    subtitleKey: "chat.preview.notes.subtitle",
    timeKey: "chat.preview.yesterday",
    unread: 1,
    messages: [
      {
        id: "notes-1",
        author: "beetle",
        textKey: "chat.preview.notes.message1",
        timeKey: "chat.preview.yesterday",
      },
    ],
  },
];

function WindowControl({
  label,
  color,
  ink,
  children,
  onClick,
}: {
  label: string;
  color: string;
  ink: string;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <Tooltip title={label}>
      <Box
        component="button"
        type="button"
        aria-label={label}
        onClick={onClick}
        sx={{
          width: 16,
          height: 16,
          p: 0,
          border: "1px solid color-mix(in srgb, var(--foreground) 10%, transparent)",
          borderRadius: "50%",
          backgroundColor: color,
          backgroundImage:
            "linear-gradient(180deg, color-mix(in srgb, #fff 54%, transparent) 0%, transparent 62%)",
          boxShadow:
            "inset 0 1px 0 color-mix(in srgb, #fff 54%, transparent), 0 1px 2px color-mix(in srgb, var(--foreground) 12%, transparent)",
          cursor: "pointer",
          appearance: "none",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: ink,
          lineHeight: 1,
          "&:hover .window-control-symbol": {
            opacity: 1,
          },
        }}
      >
        <Box
          component="svg"
          viewBox="0 0 14 14"
          className="window-control-symbol"
          sx={{
            width: 14,
            height: 14,
            opacity: 0,
            transition: "opacity var(--transition-duration) ease",
            "& path": {
              fill: "none",
              stroke: "currentColor",
              strokeWidth: 1.9,
              strokeLinecap: "round",
            },
          }}
        >
          {children}
        </Box>
      </Box>
    </Tooltip>
  );
}

export function ChatDialog({ open, onClose, onMinimize }: ChatDialogProps) {
  const { t } = useTranslation();
  const [fullScreen, setFullScreen] = useState(false);
  const [activeId, setActiveId] = useState(INITIAL_CONVERSATIONS[0].id);
  const [draft, setDraft] = useState("");
  const [conversations, setConversations] = useState(INITIAL_CONVERSATIONS);
  const activeConversation = useMemo(
    () =>
      conversations.find((conversation) => conversation.id === activeId) ??
      conversations[0],
    [activeId, conversations],
  );

  const sendDraft = () => {
    const text = draft.trim();
    if (!text) return;
    setConversations((current) =>
      current.map((conversation) =>
        conversation.id === activeConversation.id
          ? {
              ...conversation,
              subtitle: text,
              timeKey: "chat.now",
              time: undefined,
              messages: [
                ...conversation.messages,
                {
                  id: `${conversation.id}-${Date.now()}`,
                  author: "user",
                  text,
                  timeKey: "chat.now",
                },
              ],
            }
          : conversation,
      ),
    );
    setDraft("");
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth={false}
      fullScreen={fullScreen}
      aria-labelledby={CHAT_DIALOG_TITLE_ID}
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
            width: fullScreen
              ? "100vw"
              : "min(1040px, calc(100vw - 28px))",
            height: fullScreen
              ? "100vh"
              : "min(720px, calc(100vh - 96px))",
            maxWidth: "100vw",
            maxHeight: "100vh",
            m: fullScreen ? 0 : 2,
            borderRadius: fullScreen ? 0 : "var(--radius-card)",
            border: fullScreen ? "none" : "1px solid var(--form-outline-rest)",
            backgroundColor: "var(--card)",
            boxShadow: fullScreen ? "none" : "var(--os3d-content-plate-stack)",
            overflow: "hidden",
          },
        },
      }}
    >
      <Box
        sx={{
          height: "100%",
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <Box
          sx={{
            minHeight: 58,
            px: 2,
            py: 1.1,
            borderBottom:
              "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
            display: "flex",
            alignItems: "center",
            gap: 1.4,
            backgroundColor: "var(--card)",
            backgroundImage:
              "linear-gradient(180deg, color-mix(in srgb, #fff 10%, transparent) 0%, transparent 100%)",
            boxShadow: "var(--os3d-chrome-titlebar-stack)",
          }}
        >
          <Stack direction="row" spacing={0.75} alignItems="center">
            <WindowControl
              label={t("chat.close")}
              color="var(--semantic-danger)"
              ink="color-mix(in srgb, var(--semantic-danger) 62%, #491316)"
              onClick={onClose}
            >
              <path d="M4 4L10 10" />
              <path d="M10 4L4 10" />
            </WindowControl>
            <WindowControl
              label={t("chat.minimize")}
              color="var(--semantic-warning)"
              ink="color-mix(in srgb, var(--semantic-warning) 62%, #563600)"
              onClick={onMinimize}
            >
              <path d="M4 7H10" />
            </WindowControl>
            <WindowControl
              label={fullScreen ? t("chat.exitFullScreen") : t("chat.fullScreen")}
              color="var(--semantic-success)"
              ink="color-mix(in srgb, var(--semantic-success) 62%, #0b3e22)"
              onClick={() => setFullScreen((value) => !value)}
            >
              {fullScreen ? (
                <>
                  <path d="M5 3.8H3.8V5" />
                  <path d="M9 3.8H10.2V5" />
                  <path d="M5 10.2H3.8V9" />
                  <path d="M9 10.2H10.2V9" />
                </>
              ) : (
                <>
                  <path d="M4.2 6V4.2H6" />
                  <path d="M8 4.2H9.8V6" />
                  <path d="M9.8 8V9.8H8" />
                  <path d="M6 9.8H4.2V8" />
                </>
              )}
            </WindowControl>
          </Stack>
          <Typography
            id={CHAT_DIALOG_TITLE_ID}
            sx={{
              minWidth: 0,
              color: "var(--text-primary)",
              fontFamily: "var(--font-brand)",
              fontSize: "var(--font-size-body-sm)",
              fontWeight: 700,
              letterSpacing: 0,
              lineHeight: 1.2,
            }}
          >
            {t("chat.title")}
          </Typography>
        </Box>

        <Box
          sx={{
            flex: 1,
            minHeight: 0,
            display: "grid",
            gridTemplateColumns: { xs: "1fr", md: "292px minmax(0, 1fr)" },
            backgroundColor: "var(--form-group-well)",
          }}
        >
          <Box
            sx={{
              display: { xs: "none", md: "flex" },
              minHeight: 0,
              flexDirection: "column",
              borderRight:
                "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
              backgroundColor:
                "color-mix(in srgb, var(--card) 76%, var(--surface))",
            }}
          >
            <Box sx={{ px: 2, py: 1.5 }}>
              <Typography
                sx={{
                  color: "var(--text-secondary)",
                  fontSize: "var(--font-size-caption)",
                  fontWeight: 700,
                }}
              >
                {t("chat.conversationList")}
              </Typography>
            </Box>
            <Stack sx={{ px: 1.25, pb: 1.25, overflow: "auto" }} spacing={0.75}>
              {conversations.map((conversation) => {
                const selected = conversation.id === activeConversation.id;
                const title = t(conversation.titleKey);
                const subtitle =
                  conversation.subtitle ??
                  (conversation.subtitleKey ? t(conversation.subtitleKey) : "");
                const time =
                  conversation.time ??
                  (conversation.timeKey ? t(conversation.timeKey) : "");
                return (
                  <Box
                    key={conversation.id}
                    component="button"
                    type="button"
                    onClick={() => setActiveId(conversation.id)}
                    sx={{
                      width: "100%",
                      minHeight: 74,
                      p: 1.2,
                      border: selected
                        ? "1px solid color-mix(in srgb, var(--primary) 28%, var(--border))"
                        : "1px solid transparent",
                      borderRadius: "var(--radius-control)",
                      backgroundColor: selected
                        ? "color-mix(in srgb, var(--primary) 12%, var(--card))"
                        : "transparent",
                      boxShadow: selected
                        ? "var(--os3d-selection-pill-stack)"
                        : "none",
                      color: "inherit",
                      textAlign: "left",
                      cursor: "pointer",
                      display: "grid",
                      gridTemplateColumns: "42px minmax(0, 1fr) auto",
                      gap: 1,
                      alignItems: "center",
                      "&:hover": {
                        backgroundColor:
                          "color-mix(in srgb, var(--primary) 8%, var(--card))",
                      },
                    }}
                  >
                    <Box
                      sx={{
                        width: 42,
                        height: 42,
                        borderRadius: "var(--radius-control)",
                        display: "grid",
                        placeItems: "center",
                        backgroundColor: "var(--surface)",
                        position: "relative",
                      }}
                    >
                      <Os3dIcon
                        src={OS_ICON_NAV["/channels-config"]}
                        variant="inline"
                      />
                      {conversation.online ? (
                        <Box
                          sx={{
                            position: "absolute",
                            right: 2,
                            bottom: 2,
                            width: 9,
                            height: 9,
                            borderRadius: "50%",
                            backgroundColor: "var(--semantic-success)",
                            border: "2px solid var(--card)",
                          }}
                        />
                      ) : null}
                    </Box>
                    <Box sx={{ minWidth: 0 }}>
                      <Stack direction="row" spacing={0.75} alignItems="center">
                        <Typography
                          noWrap
                          sx={{
                            color: "var(--text-primary)",
                            fontSize: "var(--font-size-body-sm)",
                            fontWeight: 700,
                          }}
                        >
                          {title}
                        </Typography>
                        {conversation.unread ? (
                          <Box
                            sx={{
                              minWidth: 18,
                              height: 18,
                              px: 0.55,
                              borderRadius: "var(--radius-full)",
                              display: "grid",
                              placeItems: "center",
                              color: "var(--primary-fg)",
                              backgroundColor: "var(--primary)",
                              fontSize: 11,
                              fontWeight: 800,
                            }}
                          >
                            {conversation.unread}
                          </Box>
                        ) : null}
                      </Stack>
                      <Typography
                        noWrap
                        sx={{
                          mt: 0.4,
                          color: "var(--text-tertiary)",
                          fontSize: "var(--font-size-caption)",
                        }}
                      >
                        {subtitle}
                      </Typography>
                    </Box>
                    <Typography
                      sx={{
                        alignSelf: "start",
                        color: "var(--text-tertiary)",
                        fontSize: 11,
                        fontWeight: 700,
                      }}
                    >
                      {time}
                    </Typography>
                  </Box>
                );
              })}
            </Stack>
          </Box>

          <Box
            sx={{
              minWidth: 0,
              minHeight: 0,
              display: "flex",
              flexDirection: "column",
            }}
          >
            <Box
              sx={{
                px: { xs: 1.5, sm: 2 },
                py: 1.45,
                minHeight: 64,
                borderBottom:
                  "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
                display: "flex",
                alignItems: "center",
                gap: 1.25,
                backgroundColor: "var(--card)",
              }}
            >
              <Box
                sx={{
                  width: 42,
                  height: 42,
                  borderRadius: "var(--radius-control)",
                  display: "grid",
                  placeItems: "center",
                  backgroundColor: "var(--surface)",
                  boxShadow: "var(--os3d-icon-well-dish)",
                }}
              >
                <Os3dIcon
                  src={OS_ICON_NAV["/channels-config"]}
                  variant="inline"
                />
              </Box>
              <Box sx={{ minWidth: 0 }}>
                <Typography
                  noWrap
                  sx={{
                    color: "var(--text-primary)",
                    fontSize: "var(--font-size-body)",
                    fontWeight: 800,
                  }}
                >
                  {t(activeConversation.titleKey)}
                </Typography>
                <Typography
                  noWrap
                  sx={{
                    color: "var(--text-tertiary)",
                    fontSize: "var(--font-size-caption)",
                    fontWeight: 600,
                  }}
                >
                  {t(activeConversation.online ? "chat.online" : "chat.localPreview")}
                </Typography>
              </Box>
            </Box>

            <Stack
              spacing={1.15}
              sx={{
                flex: 1,
                minHeight: 0,
                overflow: "auto",
                p: { xs: 1.5, sm: 2.25 },
              }}
            >
              {activeConversation.messages.map((message) => {
                const isUser = message.author === "user";
                const text =
                  message.text ?? (message.textKey ? t(message.textKey) : "");
                const time =
                  message.time ?? (message.timeKey ? t(message.timeKey) : "");
                return (
                  <Box
                    key={message.id}
                    sx={{
                      display: "flex",
                      justifyContent: isUser ? "flex-end" : "flex-start",
                    }}
                  >
                    <Box
                      sx={{
                        maxWidth: { xs: "86%", sm: "68%" },
                        px: 1.45,
                        py: 1.05,
                        borderRadius: isUser
                          ? "16px 16px 5px 16px"
                          : "16px 16px 16px 5px",
                        border: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
                        backgroundColor: isUser
                          ? "color-mix(in srgb, var(--primary) 84%, var(--card))"
                          : "var(--card)",
                        color: isUser ? "var(--primary-fg)" : "var(--text-primary)",
                        boxShadow: isUser
                          ? "var(--os3d-selection-pill-stack)"
                          : "var(--os3d-control-soft-lift-stack)",
                      }}
                    >
                      <Typography
                        sx={{
                          whiteSpace: "pre-wrap",
                          overflowWrap: "anywhere",
                          fontSize: "var(--font-size-body-sm)",
                          lineHeight: 1.55,
                        }}
                      >
                        {text}
                      </Typography>
                      <Typography
                        sx={{
                          mt: 0.45,
                          color: isUser
                            ? "color-mix(in srgb, var(--primary-fg) 76%, transparent)"
                            : "var(--text-tertiary)",
                          fontSize: 11,
                          fontWeight: 700,
                          textAlign: "right",
                        }}
                      >
                        {time}
                      </Typography>
                    </Box>
                  </Box>
                );
              })}
            </Stack>

            <Box
              sx={{
                p: { xs: 1.2, sm: 1.5 },
                borderTop:
                  "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
                backgroundColor: "var(--card)",
              }}
            >
              <TextField
                fullWidth
                multiline
                maxRows={4}
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                placeholder={t("chat.inputPlaceholder")}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    sendDraft();
                  }
                }}
                InputProps={{
                  endAdornment: (
                    <InputAdornment position="end">
                      <IconButton
                        aria-label={t("chat.send")}
                        onClick={sendDraft}
                        disabled={!draft.trim()}
                        sx={{
                          color: "var(--primary)",
                          borderRadius: "var(--radius-chip)",
                        }}
                      >
                        <SendRoundedIcon fontSize="small" />
                      </IconButton>
                    </InputAdornment>
                  ),
                }}
              />
            </Box>
          </Box>
        </Box>
      </Box>
    </Dialog>
  );
}
