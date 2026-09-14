import { useCallback, useEffect, useState } from "react";
import {
  getActiveAiModelId,
  getAiSettings,
  listAiModels,
  saveAiSettings,
  setActiveAiModel,
  type AiModel,
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

/**
 * Loads + persistently manages AI config and model list.
 *
 * Two tiers:
 *  - `settings` (legacy single-row): AI provider config for installs that
 *    haven't migrated yet; used as fallback when no active model is selected.
 *  - `models` (multi-model): full list of presets + user models; `activeId`
 *    selects which one drives `ai_chat`.
 *
 * API keys are encrypted in the vault and never touch the frontend in
 * plaintext — `hasKey` only indicates one is stored. */
export function useAiConfig(): {
  settings: AiSettings;
  models: AiModel[];
  activeId: number | null;
  loading: boolean;
  reload: () => Promise<void>;
  saveSettings: (next: {
    provider: AiProvider;
    model?: string;
    baseUrl?: string;
    proxyUrl?: string;
    apiKey?: string;
    temperature: number;
  }) => Promise<void>;
  setActive: (id: number) => Promise<void>;
} {
  const [settings, setSettings] = useState<AiSettings>(DEFAULT);
  const [models, setModels] = useState<AiModel[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      const [s, m, aid] = await Promise.all([
        getAiSettings(),
        listAiModels(),
        getActiveAiModelId(),
      ]);
      setSettings(s);
      setModels(m);
      setActiveId(aid);
    } catch {
      setSettings(DEFAULT);
      setModels([]);
      setActiveId(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const saveSettings = useCallback(
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

  const setActive = useCallback(
    async (id: number) => {
      await setActiveAiModel(id);
      setActiveId(id);
    },
    []
  );

  return { settings, models, activeId, loading, reload, saveSettings, setActive };
}
