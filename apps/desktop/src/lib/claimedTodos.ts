export interface ClaimedTodo {
  id: number;
  project_id: number;
  title: string;
  claimed_at: number | null;
  lock_expiry: number;
}

export function claimedAtLabel(claimedAt: number | null): string {
  if (claimedAt === null) return 'Claim time unavailable';
  const value = new Date(claimedAt);
  if (Number.isNaN(value.getTime())) return 'Claim time unavailable';
  return `Claimed ${new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short'
  }).format(value)}`;
}
