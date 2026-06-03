// KWIC Portal Detail
function renderKwicDetail(data) {
  var c = document.getElementById('content');
  if (!data) { c.innerHTML = '<div class="error">\u30c7\u30fc\u30bf\u304c\u3042\u308a\u307e\u305b\u3093</div>'; return; }
  var h = '<div class="detail-wrap">';
  h += '<div class="course-label">KWIC \u30dd\u30fc\u30bf\u30eb</div>';
  if (data.title) h += '<div class="page-title">' + escapeHtml(data.title) + '</div>';
  if (data.date || data.sender) {
    h += '<div class="meta-table"><div class="meta-row">';
    if (data.date) h += '<span class="meta-key">\u65e5\u4ed8</span><span class="meta-value">' + escapeHtml(data.date) + '</span>';
    h += '</div>';
    if (data.sender) {
      h += '<div class="meta-row"><span class="meta-key">\u9001\u4fe1\u8005</span><span class="meta-value">' + escapeHtml(data.sender) + '</span></div>';
    }
    h += '</div>';
  }
  if (data.body_html) {
    var sanitized = data.body_html
      .replace(/<script[^>]*>[\s\S]*?<\/script>/gi, '')
      .replace(/<iframe[^>]*>[\s\S]*?<\/iframe>/gi, '')
      .replace(/<object[^>]*>[\s\S]*?<\/object>/gi, '')
      .replace(/<embed[^>]*>/gi, '')
      .replace(/<link[^>]*>/gi, '')
      .replace(/\son\w+\s*=/gi, ' data-removed=')
      .replace(/javascript\s*:/gi, 'blocked:');
    h += '<div class="section"><div class="section-body rich-text">' + sanitized + '</div></div>';
  }
  if (data.attachments && data.attachments.length) {
    h += '<div class="attachments"><h4>\u6dfb\u4ed8\u30d5\u30a1\u30a4\u30eb</h4>';
    for (var i = 0; i < data.attachments.length; i++) {
      var att = data.attachments[i];
      h += '<button class="attachment kwic-att" data-url="' + escapeHtml(att.url || '') + '">';
      h += '<span>' + ICONS.clip + ' ' + escapeHtml(att.name || '') + '</span><span style="flex:none;opacity:0.5">' + ICONS.external + '</span></button>';
    }
    h += '</div>';
  }
  h += '</div>';
  c.innerHTML = h;
  hydrateInternalLinkLabels(c);
  c.querySelectorAll('.kwic-att').forEach(function(b) {
    b.addEventListener('click', function() {
      var url = b.dataset.url;
      var name = b.textContent.trim();
      var inv = window.__TAURI__?.core?.invoke;
      if (inv && url) inv('kwic_open_link', { url: url, title: name });
    });
  });
}

function renderKwicCabinetReference(data) {
  var c = document.getElementById('content');
  if (!data) { c.innerHTML = '<div class="error">\u30c7\u30fc\u30bf\u304c\u3042\u308a\u307e\u305b\u3093</div>'; return; }
  var items = Array.isArray(data.items) ? data.items : [];
  var h = '<div class="detail-wrap">';
  h += '<div class="course-label">KWIC \u30dd\u30fc\u30bf\u30eb</div>';
  h += '<div class="page-title">' + escapeHtml(data.title || '\u5b66\u751f\u30ad\u30e3\u30d3\u30cd\u30c3\u30c8') + '</div>';
  h += '<div class="meta-table"><div class="meta-row"><span class="meta-key">\u4ef6\u6570</span><span class="meta-value">' + items.length + '</span></div></div>';
  if (!items.length) {
    h += '<div class="error">\u8a72\u5f53\u30c7\u30fc\u30bf\u306f\u3042\u308a\u307e\u305b\u3093\u3002</div>';
  } else {
    h += '<div class="attachments cabinet-reference-list"><h4>\u30ad\u30e3\u30d3\u30cd\u30c3\u30c8</h4>';
    for (var i = 0; i < items.length; i++) {
      var item = items[i] || {};
      h += '<button class="attachment kwic-cabinet-item" data-url="' + escapeHtml(item.url || '') + '" data-title="' + escapeHtml(item.name || '') + '">';
      h += '<span><span class="cabinet-item-title">' + escapeHtml(item.name || '') + '</span>';
      if (item.is_new) h += ' <span class="item-badge new">NEW</span>';
      if (item.updated_at) h += '<span class="cabinet-item-date">' + escapeHtml(item.updated_at) + '</span>';
      h += '</span><span style="flex:none;opacity:0.5">' + ICONS.external + '</span></button>';
    }
    h += '</div>';
  }
  h += '</div>';
  c.innerHTML = h;
  c.querySelectorAll('.kwic-cabinet-item').forEach(function(b) {
    b.addEventListener('click', function() {
      var url = b.dataset.url;
      var title = b.dataset.title || b.textContent.trim() || '\u5b66\u751f\u30ad\u30e3\u30d3\u30cd\u30c3\u30c8';
      var inv = window.__TAURI__?.core?.invoke;
      if (inv && url) inv('kwic_open_link', { url: url, title: title });
    });
  });
}
