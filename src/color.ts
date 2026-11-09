export const COLORS = { green: '#34c07c', amber: '#e2b93d', red: '#e8503a' } as const;

export function colorFor(pct: number, th: { amber: number; red: number }): string {
  if (pct >= th.red) return COLORS.red;
  if (pct >= th.amber) return COLORS.amber;
  return COLORS.green;
}
