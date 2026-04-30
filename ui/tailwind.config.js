/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx,html}"],
  theme: {
    extend: {
      colors: {
        // TLP semantic tokens — never used as the only signal.
        // Always paired with a glyph + text label for a11y.
        tlp: {
          green: "#16a34a",
          yellow: "#ca8a04",
          red: "#dc2626",
        },
        ocelot: {
          ink: "#0a0a0a",
          paper: "#fafaf9",
          mark: "#1f2937",
          accent: "#b45309",
        },
      },
      fontFamily: {
        display: ['"Spectral"', "ui-serif", "Georgia", "serif"],
        body: ["system-ui", "sans-serif"],
      },
    },
  },
  plugins: [],
};
