/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        primary: {
          50: "#e8f3ff",
          100: "#d1e9ff",
          200: "#a3d3ff",
          300: "#75bdff",
          400: "#47a7ff",
          500: "#1677ff",
          600: "#0958d9",
          700: "#003eb3",
          800: "#002c8c",
          900: "#001d66",
        },
        neutral: {
          50: "#fafafa",
          100: "#f5f5f5",
          200: "#eeeeee",
          300: "#e0e0e0",
          400: "#bdbdbd",
          500: "#999999",
          600: "#666666",
          700: "#333333",
          800: "#1f1f1f",
          900: "#141414",
        },
      },
      fontSize: {
        "page-title": ["20px", { lineHeight: "28px", fontWeight: "600" }],
        "section-title": ["16px", { lineHeight: "24px", fontWeight: "600" }],
        "body": ["14px", { lineHeight: "22px" }],
        "caption": ["12px", { lineHeight: "20px" }],
      },
      boxShadow: {
        "card": "0 1px 2px 0 rgba(0, 0, 0, 0.03), 0 1px 6px -1px rgba(0, 0, 0, 0.02), 0 2px 4px 0 rgba(0, 0, 0, 0.02)",
        "card-hover": "0 3px 12px 0 rgba(0, 0, 0, 0.08)",
        "dropdown": "0 6px 16px 0 rgba(0, 0, 0, 0.08), 0 9px 28px 8px rgba(0, 0, 0, 0.05)",
      },
      borderRadius: {
        "card": "8px",
      },
    },
  },
  plugins: [],
};
