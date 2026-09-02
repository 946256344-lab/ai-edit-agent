// 对齐 C:/tmp/opencut-classic/apps/web/src/components/editor/panels/assets/assets-panel-store 等面板 store
// P6 仅接入本仓 Tauri 侧 listAssetPage/getAssetEvidence 数据源，不搬 IndexedDB/OPFS
import { useCallback, useEffect, useRef, useState } from "react";

export type AssetsPanelFilter = {
  search?: string;
  kind?: "video" | "image" | "audio";
};

export function useAssetsPanelStore(
  loadPage: (params: { search?: string; offset: number; limit: number }) => Promise<{ items: unknown[]; total: number }>,
  pageSize = 30,
) {
  const [items, setItems] = useState<unknown[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const versionRef = useRef(0);

  const refresh = useCallback(async (filter: AssetsPanelFilter, offset = 0) => {
    const v = ++versionRef.current;
    setLoading(true);
    try {
      const page = await loadPage({ search: filter.search, offset, limit: pageSize });
      if (v !== versionRef.current) return;
      setItems(page.items as never);
      setTotal(page.total);
    } finally {
      if (v === versionRef.current) setLoading(false);
    }
  }, [loadPage, pageSize]);

  useEffect(() => { void refresh({}); }, [refresh]);

  return { items, total, loading, refresh };
}
