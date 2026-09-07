import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getConfig, writeConfig } from "./api/configApi";
import { cloneConfig } from "./config";
import { validateGatewayConfig } from "./configValidation";
import type { GatewayConfig } from "./types";

export function useGatewayConfig(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["config"],
    queryFn: getConfig,
    enabled: options?.enabled ?? true,
    retry: false,
  });
}

export function useConfigDumpMode() {
  return useQuery({
    queryKey: ["config_dump_mode"],
    queryFn: async () => ({ mode: "local" as const, dump: null }),
    retry: false,
    staleTime: 30_000,
  });
}

export function useUpdateConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (
      updater: (config: GatewayConfig) => GatewayConfig | void,
    ) => {
      const current =
        queryClient.getQueryData<GatewayConfig>(["config"]) ??
        (await getConfig());
      const next = cloneConfig(current);
      const returned = updater(next);
      const config = returned ?? next;
      await validateGatewayConfig(config);
      await writeConfig(config);
      return config;
    },
    onSuccess: (next) => {
      queryClient.setQueryData(["config"], next);
      void queryClient.invalidateQueries({ queryKey: ["config"] });
      void queryClient.invalidateQueries({ queryKey: ["runtime"] });
      void queryClient.invalidateQueries({ queryKey: ["config_dump"] });
      void queryClient.invalidateQueries({ queryKey: ["config_dump_mode"] });
    },
  });
}

export function useStoredStringState(key: string, defaultValue: string) {
  const [value, setValue] = useState(
    () => localStorage.getItem(key) ?? defaultValue,
  );
  useEffect(() => {
    localStorage.setItem(key, value);
  }, [key, value]);
  return [value, setValue] as const;
}
