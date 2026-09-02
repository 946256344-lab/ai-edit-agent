// 对齐 C:/tmp/opencut-classic/apps/web/src/params — 仅类型层，P1 存根
// OpenCut 的 ParamValues 为元素可动画参数的键值表，P1 先以 Record 存根，P6 再对接 Registry。
export type ParamValues = Record<string, unknown>;
export type ParamDefinition = { key: string; defaultValue: unknown };
