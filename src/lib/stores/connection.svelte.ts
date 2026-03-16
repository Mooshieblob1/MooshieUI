class ConnectionStore {
  connected = $state(false);
  serverUrl = $state("http://127.0.0.1:8188");
}

export const connection = new ConnectionStore();
