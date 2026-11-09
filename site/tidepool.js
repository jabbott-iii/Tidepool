// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// The demo page's run tabs and feature filters (ADR-0030). Ported from
// Nadir's nadir.js, without its video handling. Without this file every
// run shows, one under the other, and so does every feature.
(() => {
  'use strict';

  // ---------- Runs ----------
  const tabList = document.querySelector('[data-run-tabs]');
  const runs = [...document.querySelectorAll('[data-run]')];
  if (tabList && runs.length > 1) {
    const tabs = [...tabList.querySelectorAll('[role="tab"]')];

    // The runs become the tabs' panels.
    runs.forEach((run) => {
      run.setAttribute('role', 'tabpanel');
      run.setAttribute('aria-labelledby', `tab-${run.dataset.run}`);
    });

    const select = (index, focus) => {
      tabs.forEach((tab, i) => {
        const on = i === index;
        tab.classList.toggle('is-active', on);
        tab.setAttribute('aria-selected', String(on));
        tab.tabIndex = on ? 0 : -1;
      });
      runs.forEach((run, i) => {
        run.hidden = i !== index;
      });
      if (focus) tabs[index].focus();
    };

    tabs.forEach((tab, index) => {
      tab.addEventListener('click', () => select(index, false));
      tab.addEventListener('keydown', (event) => {
        const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
        if (event.key === 'Home' || event.key === 'End') {
          event.preventDefault();
          select(event.key === 'Home' ? 0 : tabs.length - 1, true);
        } else if (step) {
          event.preventDefault();
          select((index + step + tabs.length) % tabs.length, true);
        }
      });
    });

    tabList.hidden = false;
    select(0, false);
  }

  // ---------- Feature filters ----------
  const filters = document.querySelector('[data-feature-filters]');
  const cards = [...document.querySelectorAll('[data-features] [data-status]')];
  if (filters && cards.length) {
    const chips = [...filters.querySelectorAll('[data-status-filter]')];
    chips.forEach((chip) => chip.addEventListener('click', () => {
      const status = chip.dataset.statusFilter;
      chips.forEach((other) => {
        const on = other === chip;
        other.classList.toggle('is-active', on);
        other.setAttribute('aria-pressed', String(on));
      });
      cards.forEach((card) => {
        card.hidden = status !== 'all' && card.dataset.status !== status;
      });
    }));
    filters.hidden = false;
  }
})();
