import { useCallback, useEffect, useState } from "react";
import {
  getAiSettings,
  saveAiSettings,
  type AiProvider,
  type AiSettings,
} from "../api";

const DEFAULT: AiSettings = {
  provider: "claude",
  model: undefined,
  baseUrl: undefined,
  proxyUrl: undefined,
  hasKey: false,
  temperature: 0.7,
};

/** Loads + persists the AI provider config via IPC. Unlike the theme/font
 * hooks (localStorage), this one is backend-backed because the API key is
 * encrypted in the vault and must never touch the frontend in plaintext.
 *
 * `apiKey` on save: a non-empty value re-encrypts & overwrites; undefined /
 * empty leaves the existing key (so the user can change model without
 * re-entering the key). `proxyUrl` is stored plaintext (a proxy address
 * isn't secret; embed auth as user:pass@host if needed). */
export function useAiConfig(): {
  settings: AiSettings;
  loading: boolean;
  reload: () => Promise<void>;
  save: (next: {
    provider: AiProvider;
    model?: string;
    baseUrl?: string;
    proxyUrl?: string;
    apiKey?: string;
    temperature: number;
  }) => Promise<void>;
} {
  const [settings, setSettings] = useState<AiSettings>(DEFAULT);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      setSettings(await getAiSettings());
    } catch {
      setSettings(DEFAULT);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const save = useCallback(
    async (next: {
      provider: AiProvider;
      model?: string;
      baseUrl?: string;
      proxyUrl?: string;
      apiKey?: string;
      temperature: number;
    }) => {
      await saveAiSettings(
        next.provider,
        next.model ?? null,
        next.baseUrl ?? null,
        next.proxyUrl ?? null,
        next.apiKey ?? null,
        next.temperature
      );
      await reload();
    },
    [reload]
  );

  return { settings, loading, reload, save };
}
