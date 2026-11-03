// The backend supplies <base> at runtime so one build works at / or /heating/.
export function appUrl(path: string): string {
  const base = document.querySelector('base')?.getAttribute('href') ?? '/';
  return `${base}${path.replace(/^\//, '')}`;
}
