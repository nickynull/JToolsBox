import { ElMessage } from 'element-plus'

// ElMessage 默认 top:16px，会被 36px 的自定义标题栏压住，统一用 offset 下移
export function notify(message: string, type: 'success' | 'error' = 'success') {
  ElMessage({ message, type, offset: 48, duration: 2500 })
}
