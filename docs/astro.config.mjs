import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://docs.example.com",
  integrations: [
    starlight({
      title: "CBXShell",
      description:
        "Modern Windows Shell Extension for comic book archive thumbnails.",
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        ko: { label: "한국어", lang: "ko" }
      },
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
            { label: "Roadmap", link: "/roadmap/" }
          ]
        }
      ]
    })
  ]
});
