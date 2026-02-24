import type { Config } from 'tailwindcss';

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Industrial dark palette matching existing dashboard
        bg: {
          primary: '#0d1117',
          secondary: '#161b22',
          card: '#1c2333',
          hover: '#21283b',
        },
        border: {
          DEFAULT: '#30363d',
          light: '#484f58',
        },
        accent: {
          blue: '#58a6ff',
          green: '#3fb950',
          yellow: '#d29922',
          orange: '#db6d28',
          red: '#f85149',
          purple: '#bc8cff',
        },
        text: {
          primary: '#e6edf3',
          secondary: '#8b949e',
          muted: '#6e7681',
        },
      },
      fontFamily: {
        mono: ['"JetBrains Mono"', '"Fira Code"', 'monospace'],
      },
    },
  },
  plugins: [],
} satisfies Config;
