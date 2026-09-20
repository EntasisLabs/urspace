import { Nav } from './components/Nav'
import { Hero } from './components/Hero'
import { HowItWorks } from './components/HowItWorks'
import { Control } from './components/Control'
import { Install } from './components/Install'
import { Footer } from './components/Footer'

export default function App() {
  return (
    <>
      <Nav />
      <main>
        <Hero />
        <HowItWorks />
        <Control />
        <Install />
      </main>
      <Footer />
    </>
  )
}
