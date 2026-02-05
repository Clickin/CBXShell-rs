import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://clickin.github.io/CBXShell-rs/",
  base: "/CBXShell-rs/",
  integrations: [
    starlight({
      title: "CBXShell",
      description: "Modern Windows Shell Extension for comic book archive thumbnails.",
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        ko: { label: "한국어", lang: "ko" }
      },
      customCss: ["./src/styles/custom.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/Clickin/CBXShell-rs"
        }
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Overview", link: "/overview/" },
            { label: "Installation", link: "/installation/" },
            { label: "Build & Run", link: "/build/" }
          ]
        },
        {
          label: "Guides",
          items: [
            { label: "Configuration Manager", link: "/manager/" },
            { label: "Architecture", link: "/architecture/" },
            { label: "Packaging", link: "/packaging/" }
          ]
        },
        {
          label: "Reference",
          items: [
            { label: "Logging", link: "/logging/" },
            { label: "Project Structure", link: "/structure/" },
            { label: "FAQ & Troubleshooting", link: "/faq/" }
          ]
        }
      ]
    }),
    sitemap()
  ]
});
