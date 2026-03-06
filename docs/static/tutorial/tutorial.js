// Tutorial Engine for Rapina Documentation
// Pattern-based test validation with CodeMirror 6 editor

(function () {
  "use strict";

  // ---- State ----
  let editor = null;
  let testCases = [];
  let initialCode = "";
  let pagePath = "";
  let debounceTimer = null;
  let allPassed = false;

  // ---- Init ----
  function init() {
    const editorEl = document.getElementById("tutorial-editor");
    const codeEl = document.getElementById("tutorial-code");
    const testsEl = document.getElementById("tutorial-testcases");

    if (!editorEl || !codeEl || !testsEl) return;

    pagePath = window.location.pathname;
    initialCode = decodeHtml(codeEl.textContent).trim();
    testCases = JSON.parse(decodeHtml(testsEl.textContent));

    // Restore from localStorage or use initial code
    const savedCode = localStorage.getItem("rapina-tutorial:" + pagePath);
    const code = savedCode || initialCode;

    // Determine theme
    const isDark =
      document.documentElement.getAttribute("data-theme") === "dark";

    // Create CodeMirror editor
    editor = new CM.EditorView({
      state: CM.EditorState.create({
        doc: code,
        extensions: [
          CM.lineNumbers(),
          CM.highlightActiveLine(),
          CM.highlightActiveLineGutter(),
          CM.drawSelection(),
          CM.bracketMatching(),
          CM.indentOnInput(),
          CM.history(),
          CM.keymap.of([
            ...CM.defaultKeymap,
            ...CM.historyKeymap,
            CM.indentWithTab,
          ]),
          CM.rust(),
          isDark ? CM.rapinaDarkTheme : CM.rapinaTheme,
          CM.syntaxHighlighting(
            isDark ? CM.rapinaDarkHighlight : CM.rapinaHighlight,
          ),
          CM.EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onCodeChange();
            }
          }),
        ],
      }),
      parent: editorEl,
    });

    // Render initial test state
    renderTests();
    runTests();

    // Reset button
    const resetBtn = document.getElementById("tutorial-reset");
    if (resetBtn) {
      resetBtn.addEventListener("click", resetEditor);
    }

    // Watch for theme changes
    const observer = new MutationObserver(() => {
      const nowDark =
        document.documentElement.getAttribute("data-theme") === "dark";
      recreateEditor(nowDark);
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
  }

  function recreateEditor(isDark) {
    if (!editor) return;
    const code = editor.state.doc.toString();
    const editorEl = document.getElementById("tutorial-editor");
    editor.destroy();

    editor = new CM.EditorView({
      state: CM.EditorState.create({
        doc: code,
        extensions: [
          CM.lineNumbers(),
          CM.highlightActiveLine(),
          CM.highlightActiveLineGutter(),
          CM.drawSelection(),
          CM.bracketMatching(),
          CM.indentOnInput(),
          CM.history(),
          CM.keymap.of([
            ...CM.defaultKeymap,
            ...CM.historyKeymap,
            CM.indentWithTab,
          ]),
          CM.rust(),
          isDark ? CM.rapinaDarkTheme : CM.rapinaTheme,
          CM.syntaxHighlighting(
            isDark ? CM.rapinaDarkHighlight : CM.rapinaHighlight,
          ),
          CM.EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              onCodeChange();
            }
          }),
        ],
      }),
      parent: editorEl,
    });
  }

  // ---- Code Change Handler ----
  function onCodeChange() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      // Save to localStorage
      const code = editor.state.doc.toString();
      localStorage.setItem("rapina-tutorial:" + pagePath, code);
      runTests();
    }, 300);
  }

  // ---- Test Runner ----
  function runTests() {
    const code = editor.state.doc.toString();
    let lastMatchedResponse = null;
    let passCount = 0;

    testCases.forEach((tc, i) => {
      const regex = new RegExp(tc.pattern, "s");
      const pass = regex.test(code);
      tc._pass = pass;
      if (pass) {
        passCount++;
        if (tc.response) {
          lastMatchedResponse = tc.response;
        }
      }
    });

    renderTests();
    renderResponse(lastMatchedResponse);

    const nowAllPassed = passCount === testCases.length;
    if (nowAllPassed && !allPassed) {
      showConfetti();
    }
    allPassed = nowAllPassed;

    const completeEl = document.querySelector(".tutorial-complete");
    if (completeEl) {
      completeEl.classList.toggle("visible", allPassed);
    }
  }

  // ---- Render Tests ----
  function renderTests() {
    const container = document.getElementById("tutorial-test-list");
    if (!container) return;

    container.innerHTML = testCases
      .map((tc) => {
        const pass = tc._pass;
        return (
          '<div class="tutorial-test">' +
          '<div class="tutorial-test-icon ' +
          (pass ? "pass" : "fail") +
          '">' +
          (pass
            ? '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2"><path d="M2 6l3 3 5-5"/></svg>'
            : "") +
          "</div>" +
          '<div class="tutorial-test-text">' +
          '<div class="tutorial-test-title">' +
          escapeHtml(tc.title) +
          "</div>" +
          '<div class="tutorial-test-desc">' +
          escapeHtml(tc.description) +
          "</div>" +
          "</div>" +
          "</div>"
        );
      })
      .join("");
  }

  // ---- Render Response Preview ----
  function renderResponse(response) {
    const container = document.getElementById("tutorial-response-body");
    if (!container) return;

    if (!response) {
      container.innerHTML =
        '<div class="tutorial-response-placeholder">Complete the tests to see the response preview.</div>';
      return;
    }

    const traceId =
      "req_" + Math.random().toString(36).substring(2, 10);
    const statusClass = response.status < 400 ? "ok" : "error";
    const statusText = getStatusText(response.status);
    const bodyJson =
      typeof response.body === "string"
        ? response.body
        : JSON.stringify(response.body, null, 2);

    let html = '<div class="response-section">';
    html +=
      '<span class="response-arrow">&rarr;</span> ' +
      '<span class="response-method">' +
      response.method +
      "</span> " +
      '<span class="response-path">' +
      escapeHtml(response.path) +
      " HTTP/1.1</span><br>";
    html +=
      '&nbsp;&nbsp;<span class="response-header-name">Host:</span> ' +
      '<span class="response-header-value">localhost:3000</span>';
    html += "</div>";

    html += '<div class="response-section">';
    html +=
      '<span class="response-arrow">&larr;</span> ' +
      '<span class="response-status ' +
      statusClass +
      '">' +
      response.status +
      " " +
      statusText +
      "</span><br>";
    html +=
      '&nbsp;&nbsp;<span class="response-header-name">Content-Type:</span> ' +
      '<span class="response-header-value">application/json</span><br>';
    html +=
      '&nbsp;&nbsp;<span class="response-header-name">X-Trace-Id:</span> ' +
      '<span class="response-header-value">' +
      traceId +
      "</span>";
    html += "</div>";

    html += '<div class="response-body">' + syntaxColorJson(bodyJson) + "</div>";

    container.innerHTML = html;
  }

  // ---- JSON Syntax Coloring ----
  function syntaxColorJson(json) {
    return escapeHtml(json)
      .replace(
        /&quot;([^&]*)&quot;\s*:/g,
        '<span style="color:#2563eb">&quot;$1&quot;</span>:',
      )
      .replace(
        /:\s*&quot;([^&]*)&quot;/g,
        ': <span style="color:#059669">&quot;$1&quot;</span>',
      )
      .replace(
        /:\s*(\d+)/g,
        ': <span style="color:#d97706">$1</span>',
      )
      .replace(
        /:\s*(true|false|null)/g,
        ': <span style="color:#d97706">$1</span>',
      );
  }

  // ---- HTTP Status Codes ----
  function getStatusText(code) {
    var codes = {
      200: "OK",
      201: "Created",
      204: "No Content",
      400: "Bad Request",
      401: "Unauthorized",
      403: "Forbidden",
      404: "Not Found",
      422: "Unprocessable Entity",
      500: "Internal Server Error",
    };
    return codes[code] || "";
  }

  // ---- Reset Editor ----
  function resetEditor() {
    if (!editor) return;
    editor.dispatch({
      changes: { from: 0, to: editor.state.doc.length, insert: initialCode },
    });
    localStorage.removeItem("rapina-tutorial:" + pagePath);
    allPassed = false;
  }

  // ---- Confetti ----
  function showConfetti() {
    const container = document.createElement("div");
    container.className = "confetti-container";
    document.body.appendChild(container);

    const colors = ["#049af2", "#059669", "#8b5cf6", "#d97706", "#e11d48"];
    for (let i = 0; i < 40; i++) {
      const piece = document.createElement("div");
      piece.className = "confetti-piece";
      piece.style.left = Math.random() * 100 + "%";
      piece.style.backgroundColor =
        colors[Math.floor(Math.random() * colors.length)];
      piece.style.animationDelay = Math.random() * 0.8 + "s";
      piece.style.animationDuration = 1.5 + Math.random() * 1.5 + "s";
      piece.style.borderRadius = Math.random() > 0.5 ? "50%" : "2px";
      piece.style.width = 4 + Math.random() * 6 + "px";
      piece.style.height = 4 + Math.random() * 6 + "px";
      container.appendChild(piece);
    }

    setTimeout(() => container.remove(), 3500);
  }

  // ---- Utils ----
  function escapeHtml(text) {
    var div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  // Decode HTML entities that Zola's minifier introduces in script elements
  function decodeHtml(text) {
    var ta = document.createElement("textarea");
    ta.innerHTML = text;
    return ta.value;
  }

  // ---- Boot ----
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
