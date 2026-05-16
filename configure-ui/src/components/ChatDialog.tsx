import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import SendRoundedIcon from "@mui/icons-material/SendRounded";
import { useTranslation } from "react-i18next";
import { translateChatApiError } from "./chatErrors";
import { shouldCloseChatDialog } from "./ChatDialogModel";
import { ChatMarkdownMessage } from "./ChatMarkdownMessage";
import {
  appendMarkdownStreamDelta,
  hasVisibleMarkdownText,
  normalizeMarkdownText,
} from "./ChatMarkdownMessageModel";
import "./ChatDialog.css";
import { useDeviceApi } from "../hooks/useDeviceApi";
import type {
  ChatSessionMessage,
  ChatSessionStreamEvent,
  ChatSessionSummary,
} from "../api/endpoints/sessions";
import { OS_ICON_DASHBOARD, OS_ICON_NAV } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";

interface ChatDialogProps {
  open: boolean;
  onClose: () => void;
  onMinimize: () => void;
}

interface ChatConversation {
  id: string;
  title: string;
  subtitle?: string;
  timeKey?: string;
  time?: string;
  unread?: number;
  online?: boolean;
  messageCount?: number;
}

interface ChatMessageView {
  id: string;
  author: "beetle" | "user";
  text: string;
  timeKey?: string;
  time?: string;
  pending?: boolean;
  streaming?: boolean;
  error?: boolean;
}

const CHAT_DIALOG_TITLE_ID = "chat-dialog-title";
const DEFAULT_CHAT_ID = "configure-ui:default";
const SESSION_LIST_LIMIT = 20;
const SESSION_MESSAGE_LIMIT = 50;

function conversationFromSession(
  session: ChatSessionSummary,
  fallbackTitle: string,
): ChatConversation {
  return {
    id: session.chat_id,
    title: session.title?.trim() || fallbackTitle,
    subtitle: session.last_message?.preview ?? "",
    timeKey: session.last_message ? "chat.recent" : undefined,
    online: true,
    messageCount: session.message_count,
  };
}

function messageFromSession(message: ChatSessionMessage): ChatMessageView {
  return {
    id: message.message_id,
    author: message.role === "user" ? "user" : "beetle",
    text: normalizeMarkdownText(message.content),
  };
}

function createEmptyConversation(title: string): ChatConversation {
  return {
    id: DEFAULT_CHAT_ID,
    title,
    subtitle: "",
    online: true,
    messageCount: 0,
  };
}

function ChatStreamActivity({ label }: { label: string }) {
  return (
    <span className="chat-stream-activity" role="status" aria-live="polite">
      <span>{label}</span>
      <span className="chat-stream-activity__dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
    </span>
  );
}

function ChatStreamCaret() {
  return <span className="chat-stream-caret" aria-hidden="true" />;
}

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
  const { api, canAccessProtectedApis } = useDeviceApi();
  const [fullScreen, setFullScreen] = useState(false);
  const [activeId, setActiveId] = useState(DEFAULT_CHAT_ID);
  const [draft, setDraft] = useState("");
  const [conversations, setConversations] = useState<ChatConversation[]>(() => [
    createEmptyConversation(t("chat.defaultSessionTitle")),
  ]);
  const [messagesByChatId, setMessagesByChatId] = useState<Record<string, ChatMessageView[]>>({});
  const [sessionsLoaded, setSessionsLoaded] = useState(false);
  const [sessionNextCursor, setSessionNextCursor] = useState<string | null>(null);
  const [messageNextBeforeByChatId, setMessageNextBeforeByChatId] = useState<Record<string, string | null>>({});
  const [listLoading, setListLoading] = useState(false);
  const [listMoreLoading, setListMoreLoading] = useState(false);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [messagesMoreLoading, setMessagesMoreLoading] = useState(false);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const listRequestSeq = useRef(0);
  const detailRequestSeq = useRef(0);
  const streamAbortRef = useRef<AbortController | null>(null);
  const closeRequestedRef = useRef(false);
  const messagesScrollRef = useRef<HTMLDivElement | null>(null);
  const suppressNextAutoScrollRef = useRef(false);
  const activeConversation = useMemo(
    () =>
      conversations.find((conversation) => conversation.id === activeId) ??
      conversations[0],
    [activeId, conversations],
  );
  const activeChatId = activeConversation?.id;
  const activeMessages = activeConversation
    ? (messagesByChatId[activeConversation.id] ?? [])
    : [];
  const latestMessage = activeMessages[activeMessages.length - 1];
  const latestMessageScrollKey = latestMessage
    ? [
        activeChatId,
        activeMessages.length,
        latestMessage.id,
        latestMessage.text.length,
        latestMessage.pending ? "pending" : "settled",
        latestMessage.streaming ? "streaming" : "idle",
        latestMessage.error ? "error" : "ok",
      ].join(":")
    : `${activeChatId ?? ""}:empty`;

  const handleClose = () => {
    if (closeRequestedRef.current) return;
    closeRequestedRef.current = true;
    streamAbortRef.current?.abort();
    streamAbortRef.current = null;
    setIsStreaming(false);
    onClose();
  };

  const handleDialogClose = (_: object, reason: "backdropClick" | "escapeKeyDown") => {
    if (!shouldCloseChatDialog(reason)) return;
    handleClose();
  };

  const handleBackdropClick = () => {
    if (!shouldCloseChatDialog("backdropClick")) return;
    handleClose();
  };

  useEffect(() => {
    if (open) {
      closeRequestedRef.current = false;
      return;
    }
    streamAbortRef.current?.abort();
    streamAbortRef.current = null;
    closeRequestedRef.current = false;
  }, [open]);

  useEffect(() => {
    if (!open || !canAccessProtectedApis) return;
    const requestSeq = listRequestSeq.current + 1;
    listRequestSeq.current = requestSeq;

    const loadSessions = async () => {
      setListLoading(true);
      setSessionsLoaded(false);
      setErrorText(null);
      const result = await api.sessions.list({ limit: SESSION_LIST_LIMIT });
      if (listRequestSeq.current !== requestSeq) return;
      setListLoading(false);
      const sessionItems = result.data?.items;
      if (!result.ok || !Array.isArray(sessionItems)) {
        setErrorText(translateChatApiError(t, result.error, "chat.loadFailed"));
        setConversations([createEmptyConversation(t("chat.defaultSessionTitle"))]);
        setSessionNextCursor(null);
        setActiveId(DEFAULT_CHAT_ID);
        setSessionsLoaded(true);
        return;
      }

      const nextConversations =
        sessionItems.length > 0
          ? sessionItems.map((session) =>
              conversationFromSession(session, t("chat.defaultSessionTitle")),
            )
          : [createEmptyConversation(t("chat.defaultSessionTitle"))];

      setConversations(nextConversations);
      setActiveId((currentId) =>
        nextConversations.some((conversation) => conversation.id === currentId)
          ? currentId
          : nextConversations[0]?.id ?? DEFAULT_CHAT_ID,
      );
      setSessionNextCursor(result.data?.next_cursor ?? null);
      setSessionsLoaded(true);
    };

    void loadSessions();
  }, [api, canAccessProtectedApis, open, t]);

  useEffect(() => {
    if (!open || !canAccessProtectedApis || !sessionsLoaded || !activeChatId) return;
    const chatId = activeChatId;
    const requestSeq = detailRequestSeq.current + 1;
    detailRequestSeq.current = requestSeq;

    const loadMessages = async () => {
      setMessagesLoading(true);
      setErrorText(null);
      const result = await api.sessions.getMessages({ chatId, limit: SESSION_MESSAGE_LIMIT });
      if (detailRequestSeq.current !== requestSeq) return;
      setMessagesLoading(false);
      const messageItems = result.data?.items;
      if (!result.ok || !Array.isArray(messageItems)) {
        setErrorText(translateChatApiError(t, result.error, "chat.loadFailed"));
        setMessagesByChatId((current) => ({ ...current, [chatId]: [] }));
        return;
      }
      setMessagesByChatId((current) => ({
        ...current,
        [chatId]: messageItems.map(messageFromSession),
      }));
      setMessageNextBeforeByChatId((current) => ({
        ...current,
        [chatId]: result.data?.next_before ?? null,
      }));
    };

    void loadMessages();
  }, [activeChatId, api, canAccessProtectedApis, open, sessionsLoaded, t]);

  useEffect(() => {
    if (!open || messagesLoading) return;
    if (suppressNextAutoScrollRef.current) {
      suppressNextAutoScrollRef.current = false;
      return;
    }
    const frame = window.requestAnimationFrame(() => {
      const scrollNode = messagesScrollRef.current;
      if (!scrollNode) return;
      scrollNode.scrollTo({
        top: scrollNode.scrollHeight,
        behavior: "auto",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [latestMessageScrollKey, messagesLoading, open]);

  const upsertConversation = (chatId: string, userText: string) => {
    setConversations((current) => {
      const exists = current.some((conversation) => conversation.id === chatId);
      const next = exists
        ? current.map((conversation) =>
            conversation.id === chatId
              ? {
                  ...conversation,
                  subtitle: userText,
                  timeKey: "chat.now",
                  time: undefined,
                  messageCount: (conversation.messageCount ?? 0) + 1,
                }
              : conversation,
          )
        : [
            createEmptyConversation(t("chat.defaultSessionTitle")),
            ...current.filter((conversation) => conversation.id !== DEFAULT_CHAT_ID),
          ];
      return next.map((conversation) =>
        conversation.id === chatId
          ? {
              ...conversation,
              id: chatId,
              title: conversation.title || t("chat.defaultSessionTitle"),
              subtitle: userText,
              timeKey: "chat.now",
              online: true,
            }
          : conversation,
      );
    });
  };

  const updateMessage = (
    chatId: string,
    messageId: string,
    update: (message: ChatMessageView) => ChatMessageView,
  ) => {
    setMessagesByChatId((current) => ({
      ...current,
      [chatId]: (current[chatId] ?? []).map((message) =>
        message.id === messageId ? update(message) : message,
      ),
    }));
  };

  const appendLocalExchange = (chatId: string, userText: string, assistantId: string) => {
    const userMessage: ChatMessageView = {
      id: `local-user-${Date.now()}`,
      author: "user",
      text: userText,
      timeKey: "chat.now",
    };
    const assistantMessage: ChatMessageView = {
      id: assistantId,
      author: "beetle",
      text: "",
      timeKey: "chat.now",
      pending: true,
      streaming: true,
    };
    setMessagesByChatId((current) => ({
      ...current,
      [chatId]: [...(current[chatId] ?? []), userMessage, assistantMessage],
    }));
  };

  const applyStreamEvent = (
    chatId: string,
    assistantId: string,
    event: ChatSessionStreamEvent,
    accumulatedRef: { current: string },
  ) => {
    if (event.type === "queued") {
      updateMessage(chatId, assistantId, (message) => ({
        ...message,
        pending: !hasVisibleMarkdownText(message.text),
        streaming: true,
      }));
      return;
    }
    if (event.type === "delta") {
      const accumulated = appendMarkdownStreamDelta(accumulatedRef.current, event.delta);
      const visible = hasVisibleMarkdownText(accumulated.rendered);
      accumulatedRef.current = accumulated.raw;
      updateMessage(chatId, assistantId, (message) => ({
        ...message,
        text: accumulated.rendered,
        pending: !visible,
        streaming: true,
      }));
      return;
    }
    if (event.type === "final") {
      updateMessage(chatId, assistantId, (message) => ({
        ...message,
        id: event.messageId ?? message.id,
        text: message.text || t("chat.noResponse"),
        pending: false,
        streaming: false,
      }));
      return;
    }
    if (event.type === "error") {
      updateMessage(chatId, assistantId, (message) => ({
        ...message,
        text: translateChatApiError(t, event.error, "chat.sendFailed"),
        pending: false,
        streaming: false,
        error: true,
      }));
    }
  };

  const loadMoreSessions = async () => {
    if (!sessionNextCursor || listMoreLoading || !canAccessProtectedApis) return;
    setListMoreLoading(true);
    const result = await api.sessions.list({
      cursor: sessionNextCursor,
      limit: SESSION_LIST_LIMIT,
    });
    setListMoreLoading(false);
    const sessionItems = result.data?.items;
    if (!result.ok || !Array.isArray(sessionItems)) {
      setErrorText(translateChatApiError(t, result.error, "chat.loadFailed"));
      return;
    }
    setConversations((current) => {
      const existingIds = new Set(current.map((conversation) => conversation.id));
      const appended = sessionItems
        .map((session) => conversationFromSession(session, t("chat.defaultSessionTitle")))
        .filter((conversation) => !existingIds.has(conversation.id));
      return [...current, ...appended];
    });
    setSessionNextCursor(result.data?.next_cursor ?? null);
  };

  const loadOlderMessages = async () => {
    if (!activeChatId || messagesMoreLoading || !canAccessProtectedApis) return;
    const before = messageNextBeforeByChatId[activeChatId];
    if (!before) return;
    setMessagesMoreLoading(true);
    const result = await api.sessions.getMessages({
      chatId: activeChatId,
      before,
      limit: SESSION_MESSAGE_LIMIT,
    });
    setMessagesMoreLoading(false);
    const messageItems = result.data?.items;
    if (!result.ok || !Array.isArray(messageItems)) {
      setErrorText(translateChatApiError(t, result.error, "chat.loadFailed"));
      return;
    }
    if (messageItems.length > 0) {
      suppressNextAutoScrollRef.current = true;
    }
    setMessagesByChatId((current) => ({
      ...current,
      [activeChatId]: [
        ...messageItems.map(messageFromSession),
        ...(current[activeChatId] ?? []),
      ],
    }));
    setMessageNextBeforeByChatId((current) => ({
      ...current,
      [activeChatId]: result.data?.next_before ?? null,
    }));
  };

  const sendDraft = async () => {
    const text = draft.trim();
    if (!text || !activeConversation || isStreaming || !canAccessProtectedApis) return;
    const chatId = activeConversation.id;
    const assistantId = `local-assistant-${Date.now()}`;
    const accumulatedRef = { current: "" };
    const abortController = new AbortController();

    upsertConversation(chatId, text);
    appendLocalExchange(chatId, text, assistantId);
    setDraft("");
    setErrorText(null);
    setIsStreaming(true);
    streamAbortRef.current = abortController;

    const result = await api.sessions.streamMessage(
      { chat_id: chatId, content: text },
      (event) => applyStreamEvent(chatId, assistantId, event, accumulatedRef),
      { signal: abortController.signal },
    );

    if (streamAbortRef.current === abortController) {
      streamAbortRef.current = null;
    }
    setIsStreaming(false);
    if (abortController.signal.aborted) return;
    if (!result.ok) {
      const error = translateChatApiError(t, result.error, "chat.sendFailed");
      setErrorText(error);
      updateMessage(chatId, assistantId, (message) => ({
        ...message,
        text: error,
        pending: false,
        streaming: false,
        error: true,
      }));
      return;
    }
    updateMessage(chatId, assistantId, (message) => ({
      ...message,
      text: message.text || t("chat.noResponse"),
      pending: false,
      streaming: false,
    }));
  };

  return (
    <Dialog
      open={open}
      onClose={handleDialogClose}
      maxWidth={false}
      fullScreen={fullScreen}
      aria-labelledby={CHAT_DIALOG_TITLE_ID}
      slotProps={{
        backdrop: {
          onClick: handleBackdropClick,
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
              onClick={() => {
                if (!shouldCloseChatDialog("explicit")) return;
                handleClose();
              }}
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
            <Stack sx={{ px: 1.25, py: 1.25, overflow: "auto" }} spacing={0.75}>
              {listLoading ? (
                <Typography
                  sx={{
                    px: 1,
                    py: 1.4,
                    color: "var(--text-tertiary)",
                    fontSize: "var(--font-size-caption)",
                    fontWeight: 700,
                  }}
                >
                  {t("chat.loadingSessions")}
                </Typography>
              ) : null}
              {conversations.map((conversation) => {
                const selected = conversation.id === activeConversation.id;
                const title = conversation.title;
                const subtitle = conversation.subtitle ?? t("chat.emptySession");
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
              {sessionNextCursor ? (
                <Button
                  variant="outlined"
                  size="small"
                  disabled={listMoreLoading}
                  onClick={() => void loadMoreSessions()}
                  sx={{
                    alignSelf: "stretch",
                    minHeight: 34,
                    borderRadius: "var(--radius-control)",
                    color: "var(--primary)",
                    borderColor: "var(--form-outline-focus)",
                    fontSize: "var(--font-size-caption)",
                    fontWeight: 700,
                    textTransform: "none",
                  }}
                >
                  {listMoreLoading ? t("chat.loadingSessions") : t("chat.loadMoreSessions")}
                </Button>
              ) : null}
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
            <Stack
              ref={messagesScrollRef}
              spacing={1.15}
              sx={{
                flex: 1,
                minHeight: 0,
                overflow: "auto",
                p: { xs: 1.5, sm: 2.25 },
              }}
            >
              {messagesLoading ? (
                <Typography
                  sx={{
                    color: "var(--text-tertiary)",
                    fontSize: "var(--font-size-body-sm)",
                    fontWeight: 700,
                  }}
                >
                  {t("chat.loadingMessages")}
                </Typography>
              ) : null}
              {!messagesLoading && messageNextBeforeByChatId[activeChatId ?? ""] ? (
                <Button
                  variant="outlined"
                  size="small"
                  disabled={messagesMoreLoading}
                  onClick={() => void loadOlderMessages()}
                  sx={{
                    alignSelf: "center",
                    minHeight: 34,
                    px: 1.5,
                    borderRadius: "var(--radius-control)",
                    color: "var(--primary)",
                    borderColor: "var(--form-outline-focus)",
                    fontSize: "var(--font-size-caption)",
                    fontWeight: 700,
                    textTransform: "none",
                  }}
                >
                  {messagesMoreLoading ? t("chat.loadingMessages") : t("chat.loadOlderMessages")}
                </Button>
              ) : null}
              {!messagesLoading && activeMessages.length === 0 ? (
                <Box
                  role={errorText ? "alert" : undefined}
                  aria-live={errorText ? "polite" : undefined}
                  sx={{
                    flex: 1,
                    minHeight: 0,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    textAlign: "center",
                  }}
                >
                  <Stack
                    spacing={1.15}
                    alignItems="center"
                    sx={{
                      maxWidth: 560,
                      px: 2,
                      pb: { xs: 2, sm: 4 },
                    }}
                  >
                    {errorText ? (
                      <Box
                        sx={{
                          width: { xs: 68, sm: 82 },
                          height: { xs: 68, sm: 82 },
                        }}
                      >
                        <Os3dIcon
                          src={OS_ICON_DASHBOARD.faults}
                          alt=""
                          variant="tile"
                        />
                      </Box>
                    ) : null}
                    <Typography
                      sx={{
                        color: errorText
                          ? "color-mix(in srgb, var(--semantic-danger) 34%, var(--text-secondary))"
                          : "var(--text-tertiary)",
                        fontSize: "var(--font-size-body-sm)",
                        fontWeight: 700,
                        lineHeight: "var(--line-height-snug)",
                        overflowWrap: "anywhere",
                      }}
                    >
                      {errorText ?? t("chat.noMessages")}
                    </Typography>
                  </Stack>
                </Box>
              ) : null}
              {activeMessages.map((message) => {
                const isUser = message.author === "user";
                const showActivity =
                  !isUser &&
                  !message.error &&
                  message.pending &&
                  !hasVisibleMarkdownText(message.text);
                const showStreamingCaret =
                  !isUser &&
                  !message.error &&
                  Boolean(message.streaming) &&
                  hasVisibleMarkdownText(message.text);
                const text = message.text;
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
                      className={[
                        "chat-message-bubble",
                        !isUser && message.streaming ? "chat-message-bubble--streaming" : "",
                      ]
                        .filter(Boolean)
                        .join(" ")}
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
                        color: message.error
                          ? "var(--semantic-danger)"
                          : isUser
                            ? "var(--primary-fg)"
                            : "var(--text-primary)",
                        boxShadow: isUser
                          ? "var(--os3d-selection-pill-stack)"
                          : "var(--os3d-control-soft-lift-stack)",
                      }}
                    >
                      {showActivity ? (
                        <ChatStreamActivity label={t("chat.thinking")} />
                      ) : !isUser && !message.error ? (
                        <ChatMarkdownMessage markdown={text} />
                      ) : (
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
                      )}
                      {showStreamingCaret ? <ChatStreamCaret /> : null}
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
                        disabled={!draft.trim() || isStreaming || !canAccessProtectedApis}
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
