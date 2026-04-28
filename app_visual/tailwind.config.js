/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        // Backgrounds
        'ghost-bg':      '#0a0a0f',
        'ghost-surface': '#111118',
        'ghost-border':  '#1e1e2e',

        // Accents
        'ghost-cyan':    '#00d4ff',
        'ghost-blue':    '#4488ff',
        'ghost-yellow':  '#ffd700',

        // Position statuses
        'ghost-green':   '#00ff88',
        'ghost-red':     '#ff3366',
        'ghost-orange':  '#ff8800',

        // Text
        'ghost-text':    '#e0e0f0',
        'ghost-muted':   '#5a5a7a',
      },
      fontFamily: {
        'display': ['"Orbitron"', 'monospace'],
        'body':    ['"Inter"', 'sans-serif'],
        'mono':    ['"JetBrains Mono"', 'monospace'],
      },
      borderRadius: {
        'ghost': '12px',
      },
    },
  },
  plugins: [],
}
