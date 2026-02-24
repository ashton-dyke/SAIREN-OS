// Industrial dark palette — matches the Rust-side CSS variables.
export const colors = {
  bgPrimary: '#0d1117',
  bgSecondary: '#161b22',
  bgCard: '#1c2333',
  bgHover: '#21283b',
  border: '#30363d',
  borderLight: '#484f58',
  blue: '#58a6ff',
  green: '#3fb950',
  yellow: '#d29922',
  orange: '#db6d28',
  red: '#f85149',
  purple: '#bc8cff',
  textPrimary: '#e6edf3',
  textSecondary: '#8b949e',
  textMuted: '#6e7681',
} as const;

export function severityColor(severity: string): string {
  switch (severity.toLowerCase()) {
    case 'critical':
      return colors.red;
    case 'high':
      return colors.orange;
    case 'medium':
    case 'elevated':
      return colors.yellow;
    case 'low':
      return colors.blue;
    default:
      return colors.green;
  }
}
