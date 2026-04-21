import { createContext, type Dispatch, type SetStateAction } from "react";
import type { AsyncState } from "../types/asyncState";
import type { SoulFormState, UserFormState } from "../util/soulUserFormat";

export type SoulUserConfigContextValue = {
  ready: boolean;
  loadError: string;
  retryLoadSoul: () => void;
  retryLoadUser: () => void;
  soulForm: SoulFormState;
  setSoulForm: Dispatch<SetStateAction<SoulFormState>>;
  userForm: UserFormState;
  setUserForm: Dispatch<SetStateAction<UserFormState>>;
  soulState: AsyncState<string>;
  userState: AsyncState<string>;
  soulLoadedOnce: boolean;
  userLoadedOnce: boolean;
  soulSaveStatus: "idle" | "saving" | "ok" | "fail";
  userSaveStatus: "idle" | "saving" | "ok" | "fail";
  soulError: string;
  userError: string;
  handleSaveSoul: () => Promise<void>;
  handleSaveUser: () => Promise<void>;
  dismissSoulSaveFeedback: () => void;
  dismissUserSaveFeedback: () => void;
};

export const SoulUserConfigContext = createContext<SoulUserConfigContextValue | null>(
  null,
);
