import { useEffect } from 'react'
import { useLocation } from 'react-router-dom'

/** 路由切换时滚动到顶部，需在 Router 内调用 */
export function useScrollToTop() {
  const { pathname } = useLocation()
  useEffect(() => {
    window.scrollTo(0, 0)
    document
      .querySelectorAll('[data-app-scroll-region]')
      .forEach((el) => {
        el.scrollTop = 0
      })
    const main = document.querySelector('main[data-main-surface]')
    if (main instanceof HTMLElement) main.scrollTop = 0
  }, [pathname])
}
