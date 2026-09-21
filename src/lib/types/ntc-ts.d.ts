// ntc-ts 0.0.8 ships TypeScript sources but exposes no declarations in its exports.
declare module "ntc-ts" {
  export function getColorName(color?: string): {
    exactMatch: boolean;
    name: string;
    rgb: string | null;
  };
}
