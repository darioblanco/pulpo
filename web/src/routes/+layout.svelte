<script>
  import '../app.css';
  let { children } = $props();
</script>

<div class="app">
  <header>
    <h1>norn</h1>
  </header>
  <main>
    {@render children()}
  </main>
</div>

<style>
  .app {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  header {
    padding: 1rem;
    border-bottom: 1px solid var(--border);
  }

  header h1 {
    font-size: 1.25rem;
    font-weight: 600;
    letter-spacing: -0.025em;
  }

  main {
    flex: 1;
    padding: 1rem;
  }
</style>
