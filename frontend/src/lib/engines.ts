import { useEffect, useState } from "react";
import { speechApi, type EngineCaps } from "./speech";

let cache: EngineCaps[] | null = null;
let loading: Promise<EngineCaps[]> | null = null;

function load(): Promise<EngineCaps[]> {
  loading ??= speechApi.engines().then((r) => { cache = r.engines; return r.engines; }, (e: unknown) => { loading = null; throw e; });
  return loading;
}

/** The engines' manifests (languages, parameters, own voices), fetched once per window. */
export function useEngines(): EngineCaps[] | null {
  const [engines, setEngines] = useState<EngineCaps[] | null>(cache);
  useEffect(() => {
    if (cache) return;
    load().then(setEngines, () => undefined);
  }, []);
  return engines;
}
