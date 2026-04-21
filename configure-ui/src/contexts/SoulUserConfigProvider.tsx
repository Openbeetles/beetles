import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
import { useDeviceApi, API_ERROR } from "../hooks/useDeviceApi";
import { useUnsaved } from "../hooks/useUnsaved";
import { createAsyncState } from "../types/asyncState";
import {
  defaultSoulForm,
  defaultUserForm,
  parseSoul,
  parseUser,
  serializeSoul,
  serializeUser,
  type SoulFormState,
  type UserFormState,
} from "../util/soulUserFormat";
import {
  SoulUserConfigContext,
  type SoulUserConfigContextValue,
} from "./SoulUserConfigContext";

const MAX_CONTENT = 32 * 1024;

/** 与 `/soul-user/:tab` 路由一致，供懒加载判断当前 Tab。 */
function soulUserTabFromPathname(pathname: string): "soul" | "user" {
  const parts = pathname.split("/").filter(Boolean);
  const seg = parts[1];
  if (seg === "user") return "user";
  return "soul";
}

function apiErrorMessage(
  error: string | undefined,
  t: (k: string) => string,
): string {
  if (error === API_ERROR.PAIRING_REQUIRED)
    return t("device.pairingCodeRequired");
  return error ?? "";
}

export function SoulUserConfigProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  const { api, ready } = useDeviceApi();
  const { setDirty } = useUnsaved();

  const soulAutoFetched = useRef(false);
  const userAutoFetched = useRef(false);

  const [soulState, setSoulState] = useState(createAsyncState(""));
  const [userState, setUserState] = useState(createAsyncState(""));
  const [soulLoadedOnce, setSoulLoadedOnce] = useState(false);
  const [userLoadedOnce, setUserLoadedOnce] = useState(false);
  const [savedSoul, setSavedSoul] = useState(() =>
    serializeSoul(defaultSoulForm()),
  );
  const [savedUser, setSavedUser] = useState(() =>
    serializeUser(defaultUserForm()),
  );

  const [soulForm, setSoulForm] = useState<SoulFormState>(defaultSoulForm);
  const [userForm, setUserForm] = useState<UserFormState>(defaultUserForm);

  const [soulSaveStatus, setSoulSaveStatus] = useState<
    "idle" | "saving" | "ok" | "fail"
  >("idle");
  const [userSaveStatus, setUserSaveStatus] = useState<
    "idle" | "saving" | "ok" | "fail"
  >("idle");
  const [soulError, setSoulError] = useState("");
  const [userError, setUserError] = useState("");
  const [loadError, setLoadError] = useState("");

  const soulEffective = serializeSoul(soulForm);
  const userEffective = serializeUser(userForm);

  const loadSoul = useCallback(() => {
    if (!ready) return;
    setSoulState((prev) => ({ ...prev, loading: true, error: "" }));
    setLoadError("");
    api.soul
      .get()
      .then((res) => {
        if (res.ok) {
          const data = res.data ?? "";
          setSoulState({ data, loading: false, error: "" });
          setSoulLoadedOnce(true);
          const parsed = parseSoul(data);
          const nextForm = parsed.ok ? parsed.data : defaultSoulForm();
          setSoulForm(nextForm);
          setSavedSoul(parsed.ok ? serializeSoul(nextForm) : data);
        } else {
          setSoulState((prev) => ({ ...prev, loading: false, error: res.error ?? "" }));
          setLoadError(res.error ?? "");
        }
      })
      .catch(() => {
        setSoulState((prev) => ({ ...prev, loading: false, error: "config.errorNetwork" }));
        setLoadError("config.errorNetwork");
      });
  }, [api.soul, ready]);

  const loadUser = useCallback(() => {
    if (!ready) return;
    setUserState((prev) => ({ ...prev, loading: true, error: "" }));
    api.user
      .get()
      .then((res) => {
        if (res.ok) {
          const data = res.data ?? "";
          setUserState({ data, loading: false, error: "" });
          setUserLoadedOnce(true);
          const parsed = parseUser(data);
          const nextForm = parsed.ok ? parsed.data : defaultUserForm();
          setUserForm(nextForm);
          setSavedUser(parsed.ok ? serializeUser(nextForm) : data);
        } else {
          setUserState((prev) => ({ ...prev, loading: false, error: res.error ?? "" }));
        }
      })
      .catch(() =>
        setUserState((prev) => ({ ...prev, loading: false, error: "config.errorNetwork" })),
      );
  }, [api.user, ready]);

  const retryLoadSoul = useCallback(() => {
    setLoadError("");
    loadSoul();
  }, [loadSoul]);

  const retryLoadUser = useCallback(() => {
    loadUser();
  }, [loadUser]);

  /** 仅在进入对应 Tab 时首次自动拉取，避免打开页面就请求 SOUL+USER 两份接口。 */
  useEffect(() => {
    if (!ready) return;
    const tab = soulUserTabFromPathname(pathname);
    queueMicrotask(() => {
      if (tab === "soul" && !soulAutoFetched.current) {
        soulAutoFetched.current = true;
        loadSoul();
      }
      if (tab === "user" && !userAutoFetched.current) {
        userAutoFetched.current = true;
        loadUser();
      }
    });
  }, [ready, pathname, loadSoul, loadUser]);

  const handleSaveSoul = useCallback(async () => {
    const payload = serializeSoul(soulForm);
    if (payload.length > MAX_CONTENT) {
      setSoulSaveStatus("fail");
      setSoulError(t("config.validation.contentTooLong"));
      return;
    }
    setSoulSaveStatus("saving");
    setSoulError("");
    const res = await api.soul.save(payload);
    setSoulSaveStatus(res.ok ? "ok" : "fail");
    setSoulError(apiErrorMessage(res.error, t));
    if (res.ok) {
      setSavedSoul(payload);
    }
  }, [api.soul, soulForm, t]);

  const handleSaveUser = useCallback(async () => {
    const payload = serializeUser(userForm);
    if (payload.length > MAX_CONTENT) {
      setUserSaveStatus("fail");
      setUserError(t("config.validation.contentTooLong"));
      return;
    }
    setUserSaveStatus("saving");
    setUserError("");
    const res = await api.user.save(payload);
    setUserSaveStatus(res.ok ? "ok" : "fail");
    setUserError(apiErrorMessage(res.error, t));
    if (res.ok) {
      setSavedUser(payload);
    }
  }, [api.user, userForm, t]);

  useEffect(() => {
    setDirty(soulEffective !== savedSoul || userEffective !== savedUser);
  }, [soulEffective, savedSoul, userEffective, savedUser, setDirty]);

  const dismissSoulSaveFeedback = useCallback(() => {
    setSoulSaveStatus("idle");
    setSoulError("");
  }, []);

  const dismissUserSaveFeedback = useCallback(() => {
    setUserSaveStatus("idle");
    setUserError("");
  }, []);

  const value = useMemo<SoulUserConfigContextValue>(
    () => ({
      ready,
      loadError,
      retryLoadSoul,
      retryLoadUser,
      soulForm,
      setSoulForm,
      userForm,
      setUserForm,
      soulState,
      userState,
      soulLoadedOnce,
      userLoadedOnce,
      soulSaveStatus,
      userSaveStatus,
      soulError,
      userError,
      handleSaveSoul,
      handleSaveUser,
      dismissSoulSaveFeedback,
      dismissUserSaveFeedback,
    }),
    [
      ready,
      loadError,
      retryLoadSoul,
      retryLoadUser,
      soulForm,
      userForm,
      soulState,
      userState,
      soulLoadedOnce,
      userLoadedOnce,
      soulSaveStatus,
      userSaveStatus,
      soulError,
      userError,
      handleSaveSoul,
      handleSaveUser,
      dismissSoulSaveFeedback,
      dismissUserSaveFeedback,
    ],
  );

  return (
    <SoulUserConfigContext.Provider value={value}>
      {children}
    </SoulUserConfigContext.Provider>
  );
}
