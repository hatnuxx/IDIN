import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

// Set initial direction/language/theme before first paint
const lang = localStorage.getItem('idin.lang') || 'fa';
document.documentElement.lang = lang;
document.documentElement.dir = lang === 'fa' ? 'rtl' : 'ltr';
document.documentElement.dataset.theme = localStorage.getItem('idin.theme') || 'dark';

const app = mount(App, { target: document.getElementById('app') });

export default app;
