import {
  createContext,
  createElement,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { invokeCommand, isTauriRuntime } from './tauri';
import { isSourceApp, sourceApps, type SourceApp } from './sourceApps';

type SourceSettingsItem = {
  app: string;
  enabled: boolean;
};

type SourceSettingsResponse = {
  items: SourceSettingsItem[];
};

type SourceAppsContextValue = {
  enabledApps: SourceApp[];
  refresh: () => Promise<void>;
};

const SourceAppsContext = createContext<SourceAppsContextValue | null>(null);

export function SourceAppsProvider({ children }: { children: ReactNode }) {
  const [enabledApps, setEnabledApps] = useState<SourceApp[]>(() =>
    isTauriRuntime() ? [] : sourceApps.slice(),
  );

  const refresh = useMemo(
    () => async () => {
      if (!isTauriRuntime()) {
        setEnabledApps(sourceApps.slice());
        return;
      }

      const response = await invokeCommand<SourceSettingsResponse>('sources_list');
      const enabledSet = new Set(
        response.items
          .filter((item) => item.enabled && isSourceApp(item.app))
          .map((item) => item.app as SourceApp),
      );
      setEnabledApps(sourceApps.filter((app) => enabledSet.has(app)));
    },
    [],
  );

  useEffect(() => {
    if (!isTauriRuntime()) {
      setEnabledApps(sourceApps.slice());
      return undefined;
    }

    let cancelled = false;

    async function load() {
      try {
        if (cancelled) {
          return;
        }
        await refresh();
      } catch (error) {
        if (!cancelled) {
          console.error('failed to load enabled source apps', error);
          setEnabledApps([]);
        }
      }
    }

    void load();

    return () => {
      cancelled = true;
    };
  }, [refresh]);

  const value = useMemo<SourceAppsContextValue>(
    () => ({
      enabledApps,
      refresh,
    }),
    [enabledApps, refresh],
  );

  return createElement(SourceAppsContext.Provider, { value }, children);
}

export function useEnabledSourceApps(): SourceApp[] {
  const context = useContext(SourceAppsContext);
  if (context) {
    return context.enabledApps;
  }

  return isTauriRuntime() ? [] : sourceApps.slice();
}

export function useRefreshEnabledSourceApps(): () => Promise<void> {
  return useContext(SourceAppsContext)?.refresh ?? (() => Promise.resolve());
}
