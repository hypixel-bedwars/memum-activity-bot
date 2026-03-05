import type Style from "./style";

export type ApplyEmpty<T> = (this: T) => Style;
export type ApplyWith<T> = (this: T, style: Style) => Style;
export type Apply<T> = ApplyEmpty<T> | ApplyWith<T>;
export default Apply;
