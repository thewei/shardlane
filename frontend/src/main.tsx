import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "highlight.js/styles/github.css";
import "./theme.css";

const root = createRoot(document.getElementById("root")!);
root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
