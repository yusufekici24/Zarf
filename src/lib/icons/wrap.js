// Fluent simgesini Lucide bileşeni gibi kullanılabilir hale getirir: gelen özelliklere
// (reaktif kalarak) yalnızca "svg" eklenir.
/**
 * @param {(anchor: any, props: any) => any} Component
 * @param {string} svg
 */
export function fluentWrap(Component, svg) {
  /** @param {any} anchor @param {Record<string | symbol, any>} props */
  return function FluentIcon(anchor, props) {
    const p = new Proxy(props ?? {}, {
      get: (t, k) => (k === "svg" ? svg : Reflect.get(t, k)),
      has: (t, k) => k === "svg" || Reflect.has(t, k),
      ownKeys: (t) => [...Reflect.ownKeys(t).filter((k) => k !== "svg"), "svg"],
      getOwnPropertyDescriptor: (t, k) => (k === "svg" ? { value: svg, enumerable: true, configurable: true, writable: false } : Reflect.getOwnPropertyDescriptor(t, k)),
    });
    return Component(anchor, p);
  };
}
