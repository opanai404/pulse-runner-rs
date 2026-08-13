const state = {
  jobs: [],
  history: [],
  metrics: null,
};

const els = {
  health: document.querySelector("#health-label"),
  dot: document.querySelector(".dot"),
  total: document.querySelector("#metric-total"),
  pending: document.querySelector("#metric-pending"),
  running: document.querySelector("#metric-running"),
  succeeded: document.querySelector("#metric-succeeded"),
  failed: document.querySelector("#metric-failed"),
  jobsTable: document.querySelector("#jobs-table"),
  historyList: document.querySelector("#history-list"),
  historyCount: document.querySelector("#history-count"),
  refresh: document.querySelector("#refresh-button"),
  form: document.querySelector("#create-job-form"),
};

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(error.error || response.statusText);
  }

  return response.json();
}

async function refresh() {
  const [health, metrics, jobs, history] = await Promise.all([
    api("/api/health"),
    api("/api/metrics"),
    api("/api/jobs"),
    api("/api/history"),
  ]);

  state.metrics = metrics;
  state.jobs = jobs;
  state.history = history;
  render(health);
}

function render(health) {
  els.health.textContent = `${health.status.toUpperCase()} - ${Math.floor(health.uptime_ms / 1000)}s`;
  els.dot.classList.add("ok");

  els.total.textContent = state.metrics.total_jobs;
  els.pending.textContent = state.metrics.pending;
  els.running.textContent = state.metrics.running;
  els.succeeded.textContent = state.metrics.succeeded;
  els.failed.textContent = state.metrics.failed;

  els.jobsTable.replaceChildren(
    ...state.jobs.map((job) => {
      const tr = document.createElement("tr");
      tr.innerHTML = `
        <td>${escapeHtml(job.id)}</td>
        <td>${escapeHtml(job.kind)}</td>
        <td><span class="status ${escapeHtml(job.status)}">${escapeHtml(job.status)}</span></td>
        <td>${job.attempts}/${job.max_attempts}</td>
        <td>${formatTime(job.scheduled_for_ms)}</td>
      `;
      return tr;
    }),
  );

  const latest = [...state.history]
    .sort((a, b) => b.event.at_ms - a.event.at_ms)
    .slice(0, 24);
  els.historyCount.textContent = `${state.history.length} events`;
  els.historyList.replaceChildren(
    ...latest.map((record) => {
      const li = document.createElement("li");
      li.innerHTML = `
        <strong>${escapeHtml(record.job_id)} - ${escapeHtml(record.event.kind)}</strong>
        <span>${formatTime(record.event.at_ms)} / attempt ${record.event.attempt}: ${escapeHtml(record.event.message)}</span>
      `;
      return li;
    }),
  );
}

function formatTime(ms) {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

els.refresh.addEventListener("click", () => {
  refresh().catch(showError);
});

els.form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget);
  const key = String(data.get("idempotency_key") || "").trim();
  const body = {
    kind: data.get("kind"),
    payload: { source: "dashboard", work_units: 2 },
    max_attempts: Number(data.get("max_attempts") || 4),
  };

  if (key) {
    body.idempotency_key = key;
  }

  await api("/api/jobs", { method: "POST", body: JSON.stringify(body) });
  event.currentTarget.reset();
  await refresh();
});

function showError(error) {
  els.health.textContent = error.message;
  els.dot.classList.remove("ok");
}

refresh().catch(showError);
setInterval(() => refresh().catch(showError), 2500);
