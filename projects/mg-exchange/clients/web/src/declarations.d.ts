// CSS Modules
declare module '*.module.css' {
  const classes: Record<string, string>;
  export default classes;
}

// SVG
declare module '*.svg' {
  const content: string;
  export default content;
}
