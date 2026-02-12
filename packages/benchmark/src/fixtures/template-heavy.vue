<template>
  <div class="dashboard">
    <header class="header">
      <div class="logo">
        <img src="/logo.svg" alt="Logo">
        <h1>Dashboard</h1>
      </div>
      <nav class="nav">
        <a href="#home">Home</a>
        <a href="#analytics">Analytics</a>
        <a href="#reports">Reports</a>
        <a href="#settings">Settings</a>
      </nav>
      <div class="user-menu">
        <img src="/avatar.jpg" alt="User" class="avatar">
        <span class="username">John Doe</span>
        <button class="dropdown-toggle">▼</button>
      </div>
    </header>

    <aside class="sidebar">
      <ul class="menu">
        <li class="menu-item active">
          <span class="icon">📊</span>
          <span>Overview</span>
        </li>
        <li class="menu-item">
          <span class="icon">📈</span>
          <span>Analytics</span>
        </li>
        <li class="menu-item">
          <span class="icon">👥</span>
          <span>Users</span>
        </li>
        <li class="menu-item">
          <span class="icon">⚙️</span>
          <span>Settings</span>
        </li>
        <li class="menu-item">
          <span class="icon">📝</span>
          <span>Reports</span>
        </li>
        <li class="menu-item">
          <span class="icon">💬</span>
          <span>Messages</span>
        </li>
        <li class="menu-item">
          <span class="icon">🔔</span>
          <span>Notifications</span>
        </li>
        <li class="menu-item">
          <span class="icon">❓</span>
          <span>Help</span>
        </li>
      </ul>
    </aside>

    <main class="content">
      <section class="stats-grid">
        <div class="stat-card">
          <h3>Total Users</h3>
          <p class="stat-value">12,345</p>
          <span class="stat-change positive">+12.5%</span>
        </div>
        <div class="stat-card">
          <h3>Revenue</h3>
          <p class="stat-value">$54,321</p>
          <span class="stat-change positive">+8.2%</span>
        </div>
        <div class="stat-card">
          <h3>Orders</h3>
          <p class="stat-value">8,567</p>
          <span class="stat-change negative">-3.1%</span>
        </div>
        <div class="stat-card">
          <h3>Conversion</h3>
          <p class="stat-value">3.24%</p>
          <span class="stat-change positive">+0.5%</span>
        </div>
      </section>

      <section class="charts">
        <div class="chart-container">
          <h2>Revenue Over Time</h2>
          <div class="chart-placeholder">
            <div class="bar" style="height: 60%"></div>
            <div class="bar" style="height: 75%"></div>
            <div class="bar" style="height: 65%"></div>
            <div class="bar" style="height: 85%"></div>
            <div class="bar" style="height: 90%"></div>
            <div class="bar" style="height: 80%"></div>
            <div class="bar" style="height: 95%"></div>
          </div>
        </div>
        <div class="chart-container">
          <h2>User Growth</h2>
          <div class="chart-placeholder">
            <div class="bar" style="height: 40%"></div>
            <div class="bar" style="height: 55%"></div>
            <div class="bar" style="height: 65%"></div>
            <div class="bar" style="height: 70%"></div>
            <div class="bar" style="height: 80%"></div>
            <div class="bar" style="height: 85%"></div>
            <div class="bar" style="height: 90%"></div>
          </div>
        </div>
      </section>

      <section class="recent-activity">
        <h2>Recent Activity</h2>
        <table class="activity-table">
          <thead>
            <tr>
              <th>User</th>
              <th>Action</th>
              <th>Date</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>John Smith</td>
              <td>Created order #1234</td>
              <td>2 minutes ago</td>
              <td><span class="badge success">Completed</span></td>
            </tr>
            <tr>
              <td>Jane Doe</td>
              <td>Updated profile</td>
              <td>15 minutes ago</td>
              <td><span class="badge info">Processing</span></td>
            </tr>
            <tr>
              <td>Bob Johnson</td>
              <td>Cancelled order #1233</td>
              <td>1 hour ago</td>
              <td><span class="badge danger">Cancelled</span></td>
            </tr>
            <tr>
              <td>Alice Williams</td>
              <td>Left a review</td>
              <td>2 hours ago</td>
              <td><span class="badge success">Published</span></td>
            </tr>
            <tr>
              <td>Charlie Brown</td>
              <td>Requested refund</td>
              <td>3 hours ago</td>
              <td><span class="badge warning">Pending</span></td>
            </tr>
          </tbody>
        </table>
      </section>

      <section class="widgets">
        <div class="widget">
          <h3>Quick Actions</h3>
          <ul class="action-list">
            <li><button>Create User</button></li>
            <li><button>Generate Report</button></li>
            <li><button>Export Data</button></li>
            <li><button>Send Newsletter</button></li>
          </ul>
        </div>
        <div class="widget">
          <h3>System Status</h3>
          <ul class="status-list">
            <li><span class="indicator online"></span> API Server</li>
            <li><span class="indicator online"></span> Database</li>
            <li><span class="indicator warning"></span> Cache</li>
            <li><span class="indicator online"></span> Storage</li>
          </ul>
        </div>
        <div class="widget">
          <h3>Notifications</h3>
          <ul class="notification-list">
            <li>New user registration</li>
            <li>Payment received</li>
            <li>System backup completed</li>
            <li>New message from support</li>
          </ul>
        </div>
      </section>
    </main>

    <footer class="footer">
      <p>&copy; 2026 Dashboard. All rights reserved.</p>
      <div class="footer-links">
        <a href="#privacy">Privacy</a>
        <a href="#terms">Terms</a>
        <a href="#contact">Contact</a>
      </div>
    </footer>
  </div>
</template>

<style scoped>
.dashboard {
  display: grid;
  grid-template-areas:
    "header header"
    "sidebar content"
    "footer footer";
  grid-template-columns: 250px 1fr;
  grid-template-rows: auto 1fr auto;
  min-height: 100vh;
}

.header {
  grid-area: header;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 2rem;
  background: #fff;
  border-bottom: 1px solid #ddd;
}

.sidebar {
  grid-area: sidebar;
  background: #f8f9fa;
  padding: 1rem;
}

.content {
  grid-area: content;
  padding: 2rem;
  background: #f5f5f5;
}

.footer {
  grid-area: footer;
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
  background: #fff;
  border-top: 1px solid #ddd;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 1rem;
  margin-bottom: 2rem;
}

.stat-card {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}

.charts {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 1rem;
  margin-bottom: 2rem;
}

.chart-container {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
}

.chart-placeholder {
  display: flex;
  align-items: flex-end;
  gap: 0.5rem;
  height: 200px;
}

.bar {
  flex: 1;
  background: #4CAF50;
  border-radius: 4px 4px 0 0;
}

.recent-activity {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
  margin-bottom: 2rem;
}

.activity-table {
  width: 100%;
  border-collapse: collapse;
}

.activity-table th {
  text-align: left;
  padding: 0.75rem;
  border-bottom: 2px solid #ddd;
}

.activity-table td {
  padding: 0.75rem;
  border-bottom: 1px solid #eee;
}

.badge {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.875rem;
}

.badge.success {
  background: #d4edda;
  color: #155724;
}

.badge.danger {
  background: #f8d7da;
  color: #721c24;
}

.badge.warning {
  background: #fff3cd;
  color: #856404;
}

.badge.info {
  background: #d1ecf1;
  color: #0c5460;
}

.widgets {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 1rem;
}

.widget {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
}

.indicator {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 0.5rem;
}

.indicator.online {
  background: #4CAF50;
}

.indicator.warning {
  background: #FFC107;
}
</style>
