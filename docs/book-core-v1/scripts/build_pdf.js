const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");
const puppeteer = require("puppeteer");

function findPandoc(bookRoot) {
  const envPath = process.env.PATH || "";
  const pathParts = envPath.split(path.delimiter);
  for (const dir of pathParts) {
    if (!dir) continue;
    const cand = path.join(dir, "pandoc.exe");
    if (fs.existsSync(cand)) return cand;
  }

  const wingetPandoc = path.join(
    process.env.LOCALAPPDATA || "",
    "Microsoft",
    "WinGet",
    "Packages",
    "JohnMacFarlane.Pandoc_Microsoft.Winget.Source_8wekyb3d8bbwe",
    "pandoc-3.9.0.2",
    "pandoc.exe"
  );
  if (fs.existsSync(wingetPandoc)) return wingetPandoc;

  throw new Error(`Pandoc bulunamadi. bookRoot=${bookRoot}`);
}

function mdToHtml(bookRoot, srcFile, mode, outHtml) {
  const pandoc = findPandoc(bookRoot);
  const cssPath = path.join(bookRoot, "src", "book.css");

  const commonSources = [
    path.join(bookRoot, "src", "00-onsoz.md"),
    srcFile,
    path.join(bookRoot, "src", "cilt1-core-deep-dive.md"),
    path.join(bookRoot, "src", "cilt1-algoritma-gorsel-rehber.md"),
    path.join(bookRoot, "src", "cilt1-sozluk.md"),
  ];

  const internalsExtendedSources =
    mode === "internals"
      ? [
          path.join(bookRoot, "src", "cilt1-core-monograf-expanded.md"),
          path.join(bookRoot, "src", "cilt1-kod-ve-matematik-derinlesme.md"),
          path.join(bookRoot, "src", "cilt1-core-api-katalogu.md"),
          path.join(bookRoot, "src", "cilt1-kod-matematik-atlas-v2.md"),
          path.join(bookRoot, "src", "cilt1-core-sembol-dokumu.md"),
        ]
      : [];

  const appendixSources = [
    path.join(bookRoot, "src", "appendix-algorithm-atlas.md"),
    path.join(bookRoot, "src", "appendix-code-index.md"),
    path.join(bookRoot, "src", "appendix-source-reader.md"),
    path.join(bookRoot, "src", "appendix-math-notes.md"),
    path.join(bookRoot, "src", "appendix-kaynaklar.md"),
  ];

  const orderedSources = [
    ...commonSources,
    ...internalsExtendedSources,
    ...appendixSources,
  ].filter((f) => fs.existsSync(f));

  const args = [
    "--from",
    "markdown+pipe_tables+tex_math_dollars",
    "--to",
    "html5",
    "--standalone",
    "--toc",
    "--toc-depth",
    mode === "internals" ? "2" : "3",
    "--number-sections",
    "--metadata-file",
    path.join(bookRoot, "book-metadata.yaml"),
    "--css",
    cssPath,
    "--output",
    outHtml,
    ...orderedSources,
  ];
  execFileSync(pandoc, args, { stdio: "inherit", cwd: bookRoot });
}

async function htmlToPdf(htmlPath, pdfPath) {
  const browser = await puppeteer.launch({ headless: true, timeout: 0, protocolTimeout: 0 });
  try {
    const page = await browser.newPage();
    page.setDefaultNavigationTimeout(0);
    await page.goto(`file:///${htmlPath.replace(/\\/g, "/")}`, {
      waitUntil: "networkidle0",
      timeout: 0,
    });
    await page.pdf({
      path: pdfPath,
      format: "A4",
      printBackground: true,
      margin: {
        top: "16mm",
        right: "14mm",
        bottom: "16mm",
        left: "14mm",
      },
      preferCSSPageSize: false,
      timeout: 0,
    });
  } finally {
    await browser.close();
  }
}

async function main() {
  const bookRoot = process.argv[2];
  const outputPdf = process.argv[3];
  const mode = process.argv[4] || "internals";
  if (!bookRoot || !outputPdf) {
    throw new Error("Kullanim: node build_pdf.js <bookRoot> <outputPdf> [internals|legacy]");
  }

  const srcFile =
    mode === "legacy"
      ? path.join(bookRoot, "src", "cilt1-core.md")
      : path.join(bookRoot, "src", "cilt1-kernel-internals.md");
  const outDir = path.join(bookRoot, "out");
  if (!fs.existsSync(outDir)) {
    fs.mkdirSync(outDir, { recursive: true });
  }

  const pdfBaseName = path.basename(outputPdf, path.extname(outputPdf));
  const htmlTmp = path.join(outDir, `${pdfBaseName}.html`);
  mdToHtml(bookRoot, srcFile, mode, htmlTmp);
  await htmlToPdf(htmlTmp, outputPdf);
  console.log(`[BUILD] PDF tamamlandi: ${outputPdf}`);
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
