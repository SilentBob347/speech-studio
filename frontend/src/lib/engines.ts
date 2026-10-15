import { useEffect, useState } from "react";
import { speechApi, type EngineCaps } from "./speech";

let cache: EngineCaps[] | null = null;
let loading: Promise<EngineCaps[]> | null = null;

function load(): Promise<EngineCaps[]> {
  loading ??= speechApi.engines().then(
    (r) => { cache = r.engines; loading = null; return r.engines; },
    (e: unknown) => { loading = null; throw e; },
  );
  return loading;
}

/** The engines' manifests (languages, parameters, own voices, why one is unavailable): the last answer at once, read
 * again whenever a picker opens, since a download in Models can make an engine available. */
export function useEngines(): EngineCaps[] | null {
  const [engines, setEngines] = useState<EngineCaps[] | null>(cache);
  useEffect(() => {
    let mounted = true;
    load().then((e) => { if (mounted) setEngines(e); }, () => undefined);
    return () => { mounted = false; };
  }, []);
  return engines;
}
